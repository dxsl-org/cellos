//! Private completion boundary for permission-lowering and unmap TLB maintenance.
//!
//! A frame is released only after its private tag is invalidated locally and
//! every online hart (RISC-V via RFENCE/IPI, AArch64 via the GIC SGI) has
//! acknowledged its own all-ASID flush. Targets with a single CPU have no
//! remote to confirm and rely on the local flush alone.
//!
//! Confirming that acknowledgement is a **probe**, never a wait:
//! [`confirm_tag_invalidation`] spends one bounded deadline, and a caller that
//! does not get an acknowledgement hands its frames to
//! [`crate::memory::deferred_release`], whose reaper confirms the tag later from
//! the timer path. A release path that keeps waiting instead either eats the
//! boot's time budget or — if it gave up sooner — frees a frame a remote hart can
//! still resolve.

use crate::memory::frame::FlushCap;
use crate::memory::frame_run::{Epochs, FlushOutcome, UnconfirmedCause};
use crate::memory::paging::PAGE_SIZE;
use types::VAddr;

#[cfg(feature = "test-hooks")]
use core::sync::atomic::{AtomicBool, Ordering};

/// Test-only silent-peer window: while armed, every tag invalidation is reported
/// unconfirmed even though the request was still delivered and, on a two-hart
/// boot, genuinely flushed.
///
/// The deferred-release fixture needs the *unconfirmed* state reachable without
/// waiting for a real remote hart to stall mid-boot, and on a one-hart boot no
/// remote exists to stall at all. Arming it exercises exactly the release-path
/// branch that must retain frames instead of freeing or quarantining them;
/// disarming it is what "the acknowledgement resumes" means for the reaper.
/// Armed by the RV64 aspace fixture and the x86_64 grant fixtures.
#[cfg(feature = "test-hooks")]
static TEST_WITHHOLD_TAG_ACK: AtomicBool = AtomicBool::new(false);

/// Arm or disarm [`TEST_WITHHOLD_TAG_ACK`].
#[cfg(feature = "test-hooks")]
#[cfg_attr(
    not(all(feature = "native-domains", any(target_arch = "riscv64", target_arch = "x86_64"))),
    allow(dead_code) // reason: only the RV64 and x86_64 native-domain fixtures use it
)]
pub(crate) fn set_test_withhold_tag_ack(enabled: bool) {
    TEST_WITHHOLD_TAG_ACK.store(enabled, Ordering::Release);
}

#[cfg(feature = "test-hooks")]
pub(crate) fn test_tag_ack_withheld() -> bool {
    TEST_WITHHOLD_TAG_ACK.load(Ordering::Acquire)
}

#[cfg(feature = "test-hooks")]
static TEST_FLUSH_TRACKING: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "test-hooks")]
static TEST_FLUSHED_PAGES: crate::sync::Spinlock<alloc::vec::Vec<VAddr>> =
    crate::sync::Spinlock::ranked(
        alloc::vec::Vec::new(),
        crate::sync::rank::TEST_FLUSHED_PAGES,
    );
#[cfg(feature = "test-hooks")]
static TEST_FLUSHED_TAGS: crate::sync::Spinlock<alloc::vec::Vec<usize>> =
    crate::sync::Spinlock::ranked(alloc::vec::Vec::new(), crate::sync::rank::TEST_FLUSHED_TAGS);

/// A full shared flush ran inside the observation window: every page counts
/// as flushed.
#[cfg(feature = "test-hooks")]
static TEST_FLUSHED_ALL: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "test-hooks")]
pub(crate) fn begin_test_flush_observation() {
    TEST_FLUSHED_PAGES.lock().clear();
    TEST_FLUSHED_TAGS.lock().clear();
    TEST_FLUSHED_ALL.store(false, Ordering::Release);
    TEST_FLUSH_TRACKING.store(true, Ordering::Release);
}

#[cfg(feature = "test-hooks")]
pub(crate) fn test_flush_observed(vaddr: VAddr) -> bool {
    let page = vaddr & !(PAGE_SIZE - 1);
    TEST_FLUSHED_ALL.load(Ordering::Acquire) || TEST_FLUSHED_PAGES.lock().contains(&page)
}

/// Record a shared flush's pages for the observation window.
#[cfg(feature = "test-hooks")]
fn observe_shared_flush(ranges: &[(VAddr, usize)], all: bool) {
    if !TEST_FLUSH_TRACKING.load(Ordering::Acquire) {
        return;
    }
    if all {
        TEST_FLUSHED_ALL.store(true, Ordering::Release);
        return;
    }
    let mut pages = TEST_FLUSHED_PAGES.lock();
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::TEST_FIXTURE_SHARED,
    );
    for &(start, size) in ranges {
        pages.extend((start..start.saturating_add(size)).step_by(PAGE_SIZE));
    }
}
#[cfg(feature = "test-hooks")]
#[cfg_attr(
    not(all(feature = "native-domains", any(target_arch = "riscv64", target_arch = "x86_64"))),
    allow(dead_code) // reason: only the RV64 and x86_64 native-domain fixtures use it
)]
pub(crate) fn test_tag_flush_observed(asid: usize) -> bool {
    TEST_FLUSHED_TAGS.lock().contains(&asid)
}

#[cfg(feature = "test-hooks")]
pub(crate) fn finish_test_flush_observation() {
    TEST_FLUSH_TRACKING.store(false, Ordering::Release);
}

/// Why an invalidation could not be confirmed complete.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlushAckError {
    /// A hart did not publish its completion before the deadline.
    Timeout { hart: usize, epoch: usize },
}

/// Invalidate one private root's tag locally and confirm every online hart with
/// **one** bounded attempt.
///
/// Unlike a current-root page flush this covers a root that is inactive on the
/// caller (notably a non-current x86 PCID). RV64 remote harts acknowledge their
/// local all-ASID flush before any frame behind this tag may be released.
///
/// A caller that gets `Err` must **not** widen the wait: a peer hart can be held
/// non-preemptible for seconds, and a release path that keeps waiting either
/// burns the boot's time budget (the 25 x 200 ms budget this replaces truncated
/// two-hart boots) or — if it gave up sooner — would free a frame a remote hart
/// can still resolve. Hand the frames to
/// [`crate::memory::deferred_release`] instead and let its reaper confirm the
/// invalidation from the timer path.
pub fn confirm_tag_invalidation(asid: usize) -> Result<(), FlushAckError> {
    local_tag_flush(asid);
    // `withheld`: the failure is the test seam's, not a silent hart's.
    let mut asked = [0; crate::sched::smp::MAX_HARTS];
    #[allow(unused_variables)] // reason: only the test-hooks log below reads it
    let (result, withheld) = match await_remote_invalidation(tag_probe_ticks(), &mut asked) {
        Err(error) => (Err(error), false),
        Ok(()) => {
            // Test-only: the acknowledgement is withheld for a bounded window, so
            // the release paths' unconfirmed branch is reachable without a real
            // hart stalling mid-boot (and on a one-hart boot, where no remote
            // exists to stall).
            #[cfg(feature = "test-hooks")]
            if test_tag_ack_withheld() {
                let forced = FlushAckError::Timeout {
                    hart: crate::sched::hart_local::current_hart_id(),
                    epoch: 0,
                };
                (Err(forced), true)
            } else {
                (Ok(()), false)
            }
            #[cfg(not(feature = "test-hooks"))]
            (Ok(()), false)
        }
    };
    // The stress gate (phase 07b) counts these lines on x86 as well. A forced
    // failure names itself (`withheld (test)`), so a lane that excuses the
    // withheld window excuses only those, and a "silent hart" line is always a
    // real one.
    #[cfg(all(
        feature = "test-hooks",
        any(target_arch = "riscv64", target_arch = "x86_64")
    ))]
    if let Err(error) = &result {
        if withheld {
            log::warn!("[tlb] tag {asid} invalidation unconfirmed: withheld (test)");
        } else {
            log::warn!(
                "[tlb] tag {} invalidation unconfirmed: hart {} silent through the {} tick probe",
                asid,
                match error {
                    FlushAckError::Timeout { hart, .. } => hart,
                },
                tag_probe_ticks()
            );
        }
    }
    result
}

/// One non-blocking step of a deferred tag release: flush the tag locally and
/// ask every online remote hart again.
///
/// It is the reaper's companion to [`confirm_tag_invalidation`]. A peer that took
/// the IPI but never published its completion (or whose request was lost) only
/// finishes when it is asked again, so entries alternate between re-issuing and
/// checking. It never waits on a remote hart and costs one IPI per remote hart,
/// which is what lets the timer ISR call it. The local tag flush is repeated on
/// purpose: it is the cheapest half of the boundary, and repeating it keeps an
/// entry safe even while the remote answer is missing.
pub fn reissue_tag_invalidation(asid: usize) {
    local_tag_flush(asid);
    issue_remote_tag_flushes();
}

/// The local half of a tag invalidation, on its own.
///
/// The deferred reaper issues the remote half itself: it records the epoch each
/// remote hart was asked for *per tag*, so completion is decided by that tag's
/// own acknowledgement rather than by whether any hart owes any invalidation at
/// all (which stays true for as long as unrelated teardowns keep asking).
pub fn flush_tag_local(asid: usize) {
    local_tag_flush(asid);
}

/// The first online remote hart that still owes an invalidation it was asked
/// for, if any — a pure read of the acknowledgement machinery, no wait and no
/// flush. `None` means every remote hart has published a completion at least as
/// new as the last request, so a tag flushed before that request is safe.
pub fn tag_invalidation_outstanding() -> Option<usize> {
    // Test-only: inside the withheld window the tag reads unconfirmed even on a
    // one-hart boot, where nothing remote can be outstanding.
    #[cfg(feature = "test-hooks")]
    if test_tag_ack_withheld() {
        return Some(crate::sched::hart_local::current_hart_id());
    }
    remote_tag_flushes_outstanding()
}

/// Flush one private root's tag locally and publish the PTE stores before any
/// remote hart is asked to do the same.
fn local_tag_flush(asid: usize) {
    // Publish PTE stores before requesting an invalidation on another hart.
    crate::arch::tlb::publish_pte_stores();
    #[cfg(feature = "test-hooks")]
    if TEST_FLUSH_TRACKING.load(Ordering::Acquire) {
        let mut tags = TEST_FLUSHED_TAGS.lock();
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            None,
            crate::memory::heap::p01_owner::TEST_FIXTURE_SHARED,
        );
        tags.push(asid);
    }
    // Every leaf that differs between roots is non-global (`PageFlags::NON_GLOBAL`
    // -> `PTE_nG`), so the retiring root's own tag is the whole invalidation
    // scope. The leaves this deliberately leaves alone — the shared kernel
    // ranges and the RAM identity entries the kernel root owns — are identical
    // in every root, so a surviving entry of theirs resolves the same way.
    hal::domain::flush_asid(asid);
}

/// Ask every online remote hart to invalidate locally and wait for that answer
/// for at most `ticks`.
///
/// One request per hart with a fresh epoch: a delivered IPI can still be late
/// (the target may be in a long non-interruptible stretch), so the epoch — not
/// the firmware's return — is what proves the remote hart stopped using the
/// translation.
///
/// Broadcast, then wait over the set (phase 07b): every remote request is
/// published and its IPI sent before the first wait, and one deadline covers
/// the whole set. The targets are the online harts at the broadcast; the error
/// names the lowest-numbered hart still silent at the deadline, and `asked`
/// receives the epoch every hart was asked for. Callers keep their fail-closed
/// on that error (shared flushes quarantine their frames, tag releases defer
/// theirs) — nothing here continues past an unconfirmed hart.
#[cfg(cellos_wide)]
fn await_remote_invalidation(ticks: u64, asked: &mut Epochs) -> Result<(), FlushAckError> {
    use crate::sched::smp;
    let me = crate::sched::hart_local::current_hart_id();
    let epochs = asked;
    let requested =
        smp::RemoteEpochs::broadcast(smp::online_harts().filter(|hart| *hart != me), |hart| {
            let epoch = smp::request_tlb_flush(hart);
            epochs[hart] = epoch;
            epoch
        });
    let deadline = counter_now() + ticks;
    while let Some((hart, epoch)) = smp::tlb_flush_set_outstanding(&requested) {
        if counter_now() > deadline {
            return Err(FlushAckError::Timeout { hart, epoch });
        }
        // Spin, and only spin: this runs in whatever context the release
        // happens to be in, where widening the interrupt window can admit a
        // timer ISR into the middle of a teardown (observed as a hang in
        // the admission fixture). A remote hart makes progress anyway —
        // the emulator interleaves vCPUs — so the wait needs a budget, not
        // a yield.
        core::hint::spin_loop();
        // Two or more harts can shoot each other down at once, each with
        // interrupts possibly masked: answering our own requests here is what
        // keeps a cycle of waiters from deadlocking. Every arch needs it now
        // that hart 0 is a target too — hart 0 and hart 1 can each wait on the
        // other. The hart is re-read on every pass instead of reusing `me`:
        // callers run with interrupts either masked or not (a debug_assert
        // would be wrong for the latter), and with them unmasked a timer ISR
        // may switch this context away and resume it on another CPU.
        // Servicing a stale id would publish a completion for a hart whose TLB
        // this CPU never flushed — a forged acknowledgement — and would leave
        // the CPU we now run on (possibly in `requested`) unanswered. The read
        // and the service run with interrupts masked, so no switch can fall
        // between them; `service_tlb_flush(current)` then flushes exactly the
        // CPU it acknowledges for. Masking only narrows the interrupt window,
        // it never widens it.
        let flags = hal::arch::save_and_disable_interrupts();
        smp::answer_tlb_flush_in_spin(crate::sched::hart_local::current_hart_id());
        // SAFETY: restores exactly the interrupt state saved above.
        unsafe { hal::arch::restore_sstatus(flags) };
    }
    Ok(())
}

/// Targets without a second hart: the local flush above is the whole contract
/// and there is no remote to confirm.
#[cfg(not(cellos_wide))]
fn await_remote_invalidation(_ticks: u64, _asked: &mut Epochs) -> Result<(), FlushAckError> {
    Ok(())
}

/// Ask every online remote hart for a full local flush and return the epochs,
/// without waiting for any of them (0 = not asked).
#[cfg(cellos_wide)]
pub fn request_remote_flushes() -> Epochs {
    let me = crate::sched::hart_local::current_hart_id();
    let mut epochs = [0; crate::sched::smp::MAX_HARTS];
    for hart in crate::sched::smp::online_harts().filter(|hart| *hart != me) {
        epochs[hart] = crate::sched::smp::request_tlb_flush(hart);
    }
    epochs
}

/// No second hart: nothing to ask.
#[cfg(not(cellos_wide))]
pub fn request_remote_flushes() -> Epochs {
    [0; crate::sched::smp::MAX_HARTS]
}

/// Free-running counter used for bounded probes.
///
/// It advances whether or not the caller's context takes interrupts, which is
/// what a probe that must not wait needs.
#[cfg(cellos_wide)]
#[inline]
fn counter_now() -> u64 {
    #[cfg(target_arch = "riscv64")]
    {
        hal::common::timer::read_mtime()
    }
    #[cfg(target_arch = "aarch64")]
    {
        hal::aarch64::timer::counter_now()
    }
    #[cfg(target_arch = "x86_64")]
    {
        unsafe { core::arch::x86_64::_rdtsc() }
    }
}

#[cfg(cellos_wide)]
fn issue_remote_tag_flushes() {
    let me = crate::sched::hart_local::current_hart_id();
    for hart in crate::sched::smp::online_harts().filter(|hart| *hart != me) {
        let _ = crate::sched::smp::request_tlb_flush(hart);
    }
}

/// No second hart: nothing can owe this hart's tag invalidation.
#[cfg(not(cellos_wide))]
fn issue_remote_tag_flushes() {}

/// The first online remote hart that still owes an invalidation, if any.
#[cfg(cellos_wide)]
fn remote_tag_flushes_outstanding() -> Option<usize> {
    let me = crate::sched::hart_local::current_hart_id();
    crate::sched::smp::online_harts()
        .filter(|hart| *hart != me)
        .find(|hart| crate::sched::smp::tlb_flush_pending(*hart))
}

#[cfg(not(cellos_wide))]
fn remote_tag_flushes_outstanding() -> Option<usize> {
    None
}

/// How long [`confirm_tag_invalidation`] waits for a remote acknowledgement
/// before the caller defers its frames. Generous on purpose: a slow ack costs
/// one probe latency, a missing one costs isolation, and the probe is now the
/// *only* wait — the reaper's retries happen without blocking anyone.
#[cfg(target_arch = "riscv64")]
const fn tag_probe_ticks() -> u64 {
    20 * hal::common::timer::TICKS_PER_10MS
}

/// 200 ms in this platform's counter units — the budget for one probe.
#[cfg(target_arch = "aarch64")]
fn tag_probe_ticks() -> u64 {
    hal::aarch64::timer::counter_frequency_hz() / 5
}

#[cfg(target_arch = "x86_64")]
const fn tag_probe_ticks() -> u64 {
    // TSC ticks; exceeding this quarantines the flushed frames, so it stays
    // generous for slow (emulated) peers rather than tracking a nominal clock
    // rate.
    2_000_000_000
}

#[cfg(not(cellos_wide))]
const fn tag_probe_ticks() -> u64 {
    0
}

/// Shared-root invalidations wider than this many pages flush everything
/// locally instead of page by page (RV64 `sfence.vma`, AArch64 `tlbi vmalle1is`).
const PAGE_FLUSH_LIMIT: usize = 64;

/// Invalidate the translation for one changed page of the shared root on every
/// online hart. See [`flush_ranges`].
#[inline]
pub fn flush_page(cap: &mut FlushCap, vaddr: VAddr) -> FlushOutcome {
    flush_range(cap, vaddr & !(PAGE_SIZE - 1), PAGE_SIZE)
}

/// Invalidate a page-aligned range of the shared root. See [`flush_ranges`].
pub fn flush_range(cap: &mut FlushCap, start: VAddr, size: usize) -> FlushOutcome {
    flush_ranges(cap, &[(start, size)], false)
}

/// Invalidate page-aligned ranges of the shared (kernel) root on every online
/// hart, or every translation when `all`, as **one** operation: one remote
/// request per hart, one bounded probe.
///
/// Never panics and never waits past the probe bound. `Unconfirmed` carries the
/// epochs each remote hart was asked for: the caller keeps every frame the
/// changed translations named (`frame_run::Batch` quarantines them) and must
/// not report a permission lowering as done. Callers hold no `FRAME_ALLOCATOR`
/// (the `cap` proves it) and log the outcome themselves, because this may run
/// under `KERNEL_ROOT`, which ranks above the console.
///
/// - x86_64: every PCID and global entry locally, then every online CPU's
///   acknowledged full flush.
/// - riscv64: `sfence.vma` per page (or all), then SBI RFENCE per range. An
///   RFENCE error, or `all`, takes the IPI epoch path instead: every remote hart
///   serves the request with a full local flush.
/// - aarch64: broadcast `tlbi` per page (or `vmalle1is`); no acknowledgement
///   exists to wait for.
pub fn flush_ranges(cap: &mut FlushCap, ranges: &[(VAddr, usize)], all: bool) -> FlushOutcome {
    let _ = cap;
    #[cfg(feature = "test-hooks")]
    assert!(
        !crate::memory::frame::held_here(),
        "[tlb] shared flush while this hart holds the frame allocator"
    );
    let all = all || wider_than_page_flush(ranges);
    shared_flush(ranges, all, true)
}

/// The local half of [`flush_ranges`] plus the remote *requests*, without
/// waiting for any answer: the result is `Unconfirmed` whenever a remote hart
/// was asked. For a path that cannot wait (it holds the frame allocator); its
/// frames are quarantined until the reaper sees the epochs published.
pub fn flush_ranges_without_wait(ranges: &[(VAddr, usize)], all: bool) -> FlushOutcome {
    let all = all || wider_than_page_flush(ranges);
    shared_flush(ranges, all, false)
}

/// Does the batch cover more pages than a per-page flush should, or wrap?
fn wider_than_page_flush(ranges: &[(VAddr, usize)]) -> bool {
    let mut pages = 0usize;
    for &(start, size) in ranges {
        debug_assert!(start & (PAGE_SIZE - 1) == 0);
        debug_assert!(size & (PAGE_SIZE - 1) == 0);
        if start.checked_add(size).is_none() {
            // A wrapping range is a caller bug; invalidate everything rather
            // than trust its bounds.
            return true;
        }
        pages = pages.saturating_add(size / PAGE_SIZE);
    }
    pages > PAGE_FLUSH_LIMIT
}

/// What remote harts still owe after the arch half of a shared flush
/// (`crate::arch::tlb::shared_flush_local`) has run.
pub(crate) enum RemoteWork {
    /// Nothing: the arch's own instructions reached every hart (AArch64
    /// broadcast `tlbi`, a completed RV64 RFENCE) or there is no other hart.
    // reason: x86 always takes the epoch path.
    #[allow(dead_code)]
    None,
    /// Every online remote hart must serve the IPI epoch with a full flush.
    /// `rfence_failed`: RV64 tried RFENCE first and the firmware refused.
    // reason: AArch64's broadcast `tlbi` never needs it.
    #[allow(dead_code)]
    EpochPath { rfence_failed: bool },
}

fn shared_flush(ranges: &[(VAddr, usize)], all: bool, wait: bool) -> FlushOutcome {
    #[cfg(feature = "test-hooks")]
    observe_shared_flush(ranges, all);
    match crate::arch::tlb::shared_flush_local(ranges, all) {
        RemoteWork::None => FlushOutcome::Confirmed,
        RemoteWork::EpochPath { rfence_failed } => remote_full_flush(wait, rfence_failed),
    }
}

/// Every online remote hart's full flush: one bounded probe when `wait`,
/// otherwise only the requests (`Unconfirmed { NotAwaited }` whenever one was
/// sent: nobody waited, so no hart is known to be silent).
fn remote_full_flush(wait: bool, rfence_failed: bool) -> FlushOutcome {
    if wait {
        let mut epochs = [0; crate::sched::smp::MAX_HARTS];
        return match await_remote_invalidation(tag_probe_ticks(), &mut epochs) {
            Ok(()) => FlushOutcome::Confirmed,
            Err(FlushAckError::Timeout { hart, .. }) => FlushOutcome::Unconfirmed {
                epochs,
                cause: if rfence_failed {
                    UnconfirmedCause::RfenceFailed { hart }
                } else {
                    UnconfirmedCause::Timeout { hart }
                },
            },
        };
    }
    let epochs = request_remote_flushes();
    match epochs.iter().position(|epoch| *epoch != 0) {
        None => FlushOutcome::Confirmed,
        Some(hart) => FlushOutcome::Unconfirmed {
            epochs,
            cause: UnconfirmedCause::NotAwaited { hart: Some(hart) },
        },
    }
}
