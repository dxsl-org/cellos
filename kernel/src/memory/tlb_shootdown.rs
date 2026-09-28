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

use crate::memory::paging::PAGE_SIZE;
use types::VAddr;

#[cfg(feature = "test-hooks")]
use core::sync::atomic::{AtomicBool, Ordering};

/// Test-only negative control: pretend the peer's acknowledgement never arrives.
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
static TEST_SKIP_REMOTE_RFENCE: AtomicBool = AtomicBool::new(false);

/// Test-only silent-peer window: while armed, every tag invalidation is reported
/// unconfirmed even though the request was still delivered and, on a two-hart
/// boot, genuinely flushed.
///
/// The deferred-release fixture needs the *unconfirmed* state reachable without
/// waiting for a real remote hart to stall mid-boot, and on a one-hart boot no
/// remote exists to stall at all. Arming it exercises exactly the release-path
/// branch that must retain frames instead of freeing or quarantining them;
/// disarming it is what "the acknowledgement resumes" means for the reaper.
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
static TEST_WITHHOLD_TAG_ACK: AtomicBool = AtomicBool::new(false);

/// Arm or disarm [`TEST_WITHHOLD_TAG_ACK`].
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub(crate) fn set_test_withhold_tag_ack(enabled: bool) {
    TEST_WITHHOLD_TAG_ACK.store(enabled, Ordering::Release);
}

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
fn test_tag_ack_withheld() -> bool {
    TEST_WITHHOLD_TAG_ACK.load(Ordering::Acquire)
}

#[cfg(feature = "test-hooks")]
static TEST_FLUSH_TRACKING: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "test-hooks")]
static TEST_FLUSHED_PAGES: crate::sync::Spinlock<alloc::vec::Vec<VAddr>> =
    crate::sync::Spinlock::new(alloc::vec::Vec::new());
#[cfg(feature = "test-hooks")]
static TEST_FLUSHED_TAGS: crate::sync::Spinlock<alloc::vec::Vec<usize>> =
    crate::sync::Spinlock::new(alloc::vec::Vec::new());

#[cfg(feature = "test-hooks")]
pub(crate) fn begin_test_flush_observation() {
    TEST_FLUSHED_PAGES.lock().clear();
    TEST_FLUSHED_TAGS.lock().clear();
    TEST_FLUSH_TRACKING.store(true, Ordering::Release);
}

#[cfg(feature = "test-hooks")]
pub(crate) fn test_flush_observed(vaddr: VAddr) -> bool {
    let page = vaddr & !(PAGE_SIZE - 1);
    TEST_FLUSHED_PAGES.lock().contains(&page)
}
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub(crate) fn test_tag_flush_observed(asid: usize) -> bool {
    TEST_FLUSHED_TAGS.lock().contains(&asid)
}

#[cfg(feature = "test-hooks")]
pub(crate) fn finish_test_flush_observation() {
    TEST_FLUSH_TRACKING.store(false, Ordering::Release);
}

/// Enable the test-only negative control around one known self-test operation.
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub(crate) fn set_test_skip_remote_rfence(enabled: bool) {
    TEST_SKIP_REMOTE_RFENCE.store(enabled, Ordering::Release);
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
    let result = match await_remote_invalidation(tag_probe_ticks()) {
        Err(error) => Err(error),
        Ok(()) => {
            // Test-only: the acknowledgement is withheld for a bounded window, so
            // the release paths' unconfirmed branch is reachable without a real
            // hart stalling mid-boot (and on a one-hart boot, where no remote
            // exists to stall).
            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            if test_tag_ack_withheld() {
                Err(FlushAckError::Timeout {
                    hart: crate::task::hart_local::current_hart_id(),
                    epoch: 0,
                })
            } else {
                Ok(())
            }
            #[cfg(not(all(feature = "test-hooks", target_arch = "riscv64")))]
            Ok(())
        }
    };
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    if let Err(error) = &result {
        log::warn!(
            "[tlb] tag {} invalidation unconfirmed: hart {} silent through the {} tick probe",
            asid,
            match error {
                FlushAckError::Timeout { hart, .. } => hart,
            },
            tag_probe_ticks()
        );
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

/// The first online remote hart that still owes an invalidation it was asked
/// for, if any — a pure read of the acknowledgement machinery, no wait and no
/// flush. `None` means every remote hart has published a completion at least as
/// new as the last request, so a tag flushed before that request is safe.
pub fn tag_invalidation_outstanding() -> Option<usize> {
    // Test-only: inside the withheld window the tag reads unconfirmed even on a
    // one-hart boot, where nothing remote can be outstanding.
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    if test_tag_ack_withheld() {
        return Some(crate::task::hart_local::current_hart_id());
    }
    remote_tag_flushes_outstanding()
}

/// Flush one private root's tag locally and publish the PTE stores before any
/// remote hart is asked to do the same.
fn local_tag_flush(asid: usize) {
    // Publish PTE stores before requesting an invalidation on another hart.
    #[cfg(target_arch = "riscv64")]
    {
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
        // SAFETY: an S-mode fence orders prior page-table stores.
        unsafe { core::arch::asm!("fence rw, rw", options(nostack)) };
    }
    #[cfg(target_arch = "aarch64")]
    {
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
        // SAFETY: `dsb ishst` orders prior page-table stores before the IPI that
        // asks a remote hart to invalidate them.
        unsafe { core::arch::asm!("dsb ishst", options(nostack)) };
    }
    #[cfg(feature = "test-hooks")]
    if TEST_FLUSH_TRACKING.load(Ordering::Acquire) {
        TEST_FLUSHED_TAGS.lock().push(asid);
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
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
fn await_remote_invalidation(ticks: u64) -> Result<(), FlushAckError> {
    use crate::task::smp;
    let me = crate::task::hart_local::current_hart_id();
    for hart in smp::online_harts().filter(|hart| *hart != me) {
        let epoch = smp::request_tlb_flush(hart);
        let deadline = counter_now() + ticks;
        while !smp::tlb_flush_completed(hart, epoch) {
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
        }
    }
    Ok(())
}

/// Targets without a second hart: the local flush above is the whole contract
/// and there is no remote to confirm.
#[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
fn await_remote_invalidation(_ticks: u64) -> Result<(), FlushAckError> {
    Ok(())
}

/// Free-running counter used for bounded probes.
///
/// It advances whether or not the caller's context takes interrupts, which is
/// what a probe that must not wait needs.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
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
}

#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
fn issue_remote_tag_flushes() {
    let me = crate::task::hart_local::current_hart_id();
    for hart in crate::task::smp::online_harts().filter(|hart| *hart != me) {
        let _ = crate::task::smp::request_tlb_flush(hart);
    }
}

/// No second hart: nothing can owe this hart's tag invalidation.
#[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
fn issue_remote_tag_flushes() {}

/// The first online remote hart that still owes an invalidation, if any.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
fn remote_tag_flushes_outstanding() -> Option<usize> {
    let me = crate::task::hart_local::current_hart_id();
    crate::task::smp::online_harts()
        .filter(|hart| *hart != me)
        .find(|hart| crate::task::smp::tlb_flush_pending(*hart))
}

#[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
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

#[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
const fn tag_probe_ticks() -> u64 {
    0
}

/// Invalidate the translation for one changed page before its memory can run or reuse.
#[inline]
pub fn flush_page(vaddr: VAddr) {
    let page = vaddr & !(PAGE_SIZE - 1);
    #[cfg(feature = "test-hooks")]
    if TEST_FLUSH_TRACKING.load(Ordering::Acquire) {
        TEST_FLUSHED_PAGES.lock().push(page);
    }
    flush_range(page, PAGE_SIZE);
}

/// Invalidate a page-aligned range after its PTEs have changed.
pub fn flush_range(start: VAddr, size: usize) {
    debug_assert!(start & (PAGE_SIZE - 1) == 0);
    debug_assert!(size & (PAGE_SIZE - 1) == 0);
    let end = start
        .checked_add(size)
        .expect("TLB flush range must not wrap the address space");

    #[cfg(target_arch = "riscv64")]
    {
        // Keep the PTE write visible to both the compiler and remote table walkers
        // before the firmware asks those harts to execute SFENCE.VMA.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
        // SAFETY: S-mode may order prior page-table stores with `fence`; this
        // changes no memory and is required before remote invalidation.
        unsafe {
            core::arch::asm!("fence rw, rw", options(nostack));
        }

        for page in (start..end).step_by(PAGE_SIZE) {
            hal::paging::flush_tlb_page(page);
        }
        let Some((remote_mask, remote_base)) = crate::task::smp::remote_online_sbi_target() else {
            return;
        };
        #[cfg(feature = "test-hooks")]
        if TEST_SKIP_REMOTE_RFENCE.load(Ordering::Acquire) {
            return;
        }
        if let Err(error) = hal::sbi::sbi_remote_sfence_vma(remote_mask, remote_base, start, size) {
            // Continuing would allow a remote hart to retain write access to a
            // page that the current hart has already restricted or retired.
            panic!("[tlb] RV64 RFENCE failed after PTE update: {}", error);
        }
    }

    #[cfg(not(any(target_arch = "riscv64", target_arch = "riscv32")))]
    for page in (start..end).step_by(PAGE_SIZE) {
        hal::paging::flush_tlb_page(page);
    }

    #[cfg(target_arch = "riscv32")]
    {
        let _ = end;
    }
}
