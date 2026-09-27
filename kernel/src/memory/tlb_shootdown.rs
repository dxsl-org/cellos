//! Private completion boundary for permission-lowering and unmap TLB maintenance.
//!
//! No caller may execute newly restricted code or recycle a retired VA/frame
//! until this function returns. RV64 uses SBI RFENCE for every online remote
//! hart; other targets retain their established local or broadcast HAL paths.

use crate::memory::paging::PAGE_SIZE;
use types::VAddr;

#[cfg(feature = "test-hooks")]
use core::sync::atomic::{AtomicBool, Ordering};

/// Test-only negative-control switch. It is absent from production kernels.
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
static TEST_SKIP_REMOTE_RFENCE: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "test-hooks")]
static TEST_FLUSH_TRACKING: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "test-hooks")]
static TEST_FLUSHED_PAGES: crate::sync::Spinlock<alloc::vec::Vec<VAddr>> =
    crate::sync::Spinlock::new(alloc::vec::Vec::new());

#[cfg(feature = "test-hooks")]
pub(crate) fn begin_test_flush_observation() {
    TEST_FLUSHED_PAGES.lock().clear();
    TEST_FLUSH_TRACKING.store(true, Ordering::Release);
}

#[cfg(feature = "test-hooks")]
pub(crate) fn test_flush_observed(vaddr: VAddr) -> bool {
    let page = vaddr & !(PAGE_SIZE - 1);
    TEST_FLUSHED_PAGES.lock().contains(&page)
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

/// Invalidate one ASID everywhere and **wait for every online hart to confirm**.
///
/// This is the release-side contract: a tag (and the frames behind it) may only
/// be recycled once no hart can still resolve a translation under it. A firmware
/// call returning is not that evidence — the target performs its own flush on the
/// way through the switch boundary and publishes an epoch this waits on.
///
/// On failure the caller must keep the tag and the frames: a stale entry
/// resolving inside a successor root is exactly the failure this prevents.
pub fn flush_asid_and_await(asid: usize) -> Result<(), FlushAckError> {
    // Local first: this hart must stop using the tag before it asks anyone else.
    hal::domain::flush_asid(asid);
    await_remote_invalidation("asid")
}

/// Invalidate a page-aligned range everywhere and **wait for every online hart**.
///
/// The frame-release contract for unmapping: the leaf and any pruned table frames
/// may only return to the allocator once no hart can still walk the translation
/// that pointed at them. `flush_range` remains the broadcast-and-hope variant for
/// callers that release nothing.
pub fn flush_range_and_await(start: VAddr, size: usize) -> Result<(), FlushAckError> {
    debug_assert!(start & (PAGE_SIZE - 1) == 0);
    debug_assert!(size & (PAGE_SIZE - 1) == 0);
    let end = start
        .checked_add(size)
        .expect("TLB flush range must not wrap the address space");
    for page in (start..end).step_by(PAGE_SIZE) {
        // The observation window must see every invalidation path, not just the
        // one that happens to be named `flush_page`: a fixture that asserts "the
        // page was flushed before the frame was released" would otherwise pass or
        // fail on which function the caller used.
        #[cfg(feature = "test-hooks")]
        if TEST_FLUSH_TRACKING.load(Ordering::Acquire) {
            TEST_FLUSHED_PAGES.lock().push(page);
        }
        hal::paging::flush_tlb_page(page);
    }
    await_remote_invalidation("range")
}

/// Ask every online remote hart to invalidate locally and wait for each one.
///
/// One request per hart, retried with a fresh epoch: a delivered IPI can still be
/// late (the target may be in a long non-interruptible stretch), and failing closed
/// costs a leaked tag or retained frames while a retry costs one IPI.
#[cfg(target_arch = "riscv64")]
fn await_remote_invalidation(what: &str) -> Result<(), FlushAckError> {
    use crate::task::smp;
    let me = crate::task::hart_local::current_hart_id();
    for hart in smp::online_harts().filter(|hart| *hart != me) {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let epoch = smp::request_tlb_flush(hart);
            let deadline = hal::common::timer::read_mtime() + TLB_ACK_TIMEOUT_TICKS;
            let mut spins = 0usize;
            let _ = &mut spins;
            while !smp::tlb_flush_completed(hart, epoch) {
                if hal::common::timer::read_mtime() > deadline {
                    break;
                }
                // Spin, and only spin: this runs in whatever context the release
                // happens to be in, where widening the interrupt window can admit a
                // timer ISR into the middle of a teardown (observed as a hang in
                // the admission fixture). A remote hart makes progress anyway —
                // the emulator interleaves vCPUs — so the wait needs a budget, not
                // a yield.
                spins = spins.wrapping_add(1);
                core::hint::spin_loop();
            }
            if smp::tlb_flush_completed(hart, epoch) {
                break;
            }
            #[cfg(feature = "test-hooks")]
            log::warn!(
                "[tlb] {} invalidation unacknowledged on hart {} (attempt {})",
                what,
                hart,
                attempt
            );
            if attempt >= TLB_ACK_ATTEMPTS {
                return Err(FlushAckError::Timeout { hart, epoch });
            }
        }
    }
    Ok(())
}

/// Non-RV64: the backends are single-CPU, so the local flush above is the whole
/// contract and there is no remote to confirm.
#[cfg(not(target_arch = "riscv64"))]
fn await_remote_invalidation(_what: &str) -> Result<(), FlushAckError> {
    Ok(())
}

/// How long one [`flush_asid_and_await`] attempt waits before re-issuing the
/// request. Generous on purpose: a slow ack costs latency, a missing one costs
/// isolation, so the retry loop — not the deadline — is what bounds the wait.
#[cfg(target_arch = "riscv64")]
const TLB_ACK_TIMEOUT_TICKS: u64 = 20 * hal::common::timer::TICKS_PER_10MS;

/// How many times a request is re-issued before the release fails closed.
///
/// Twenty-five attempts at ~200 ms is a five-second bound. It has to cover a remote hart
/// that is in a long non-preemptible stretch — at boot that is the secondary's
/// own selftest, which demonstrably outlasted a three-attempt budget — while still
/// being finite. Phase 03's revoke should *defer* the release to a reaper instead
/// of widening this bound further.
#[cfg(target_arch = "riscv64")]
const TLB_ACK_ATTEMPTS: usize = 25;

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
