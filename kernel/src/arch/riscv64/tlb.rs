// SPDX-License-Identifier: MPL-2.0
//! `tlb`: local TLB flush and ordering page-table stores before a shootdown.

/// Flush this hart's TLB: `sfence.vma`, all ASIDs.
#[inline(always)]
pub(crate) fn flush_all() {
    // SAFETY: sfence.vma is a privileged S-mode fence; always safe from S-mode.
    unsafe {
        core::arch::asm!("sfence.vma", options(nostack));
    }
}

/// Order prior page-table stores before asking another hart to invalidate.
pub(crate) fn publish_pte_stores() {
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
    // SAFETY: an S-mode fence orders prior page-table stores.
    unsafe { core::arch::asm!("fence rw, rw", options(nostack)) };
}

/// Test-only negative control: pretend the peer's acknowledgement never
/// arrives (the RFENCE is skipped and the flush reports success).
#[cfg(feature = "test-hooks")]
static TEST_SKIP_REMOTE_RFENCE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Enable the test-only negative control around one known self-test operation.
#[cfg(feature = "test-hooks")]
pub(crate) fn set_test_skip_remote_rfence(enabled: bool) {
    TEST_SKIP_REMOTE_RFENCE.store(enabled, core::sync::atomic::Ordering::Release);
}

/// RFENCE errors seen by shared flushes; each fell back to the IPI epoch path.
/// Logged by [`report_rfence_failures`] outside any lock.
static RFENCE_FAILURES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static RFENCE_LAST_ERROR: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static RFENCE_REPORTED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// The local half of a shared-root flush (`tlb_shootdown::flush_ranges`):
/// `sfence.vma` per page (or all), then SBI RFENCE per range (or one over the
/// whole address space). Only an RFENCE error leaves the remote harts to the
/// IPI epoch path, which each serves with a full local flush.
pub(crate) fn shared_flush_local(
    ranges: &[(types::VAddr, usize)],
    all: bool,
) -> crate::memory::tlb_shootdown::RemoteWork {
    use crate::memory::tlb_shootdown::RemoteWork;
    // Keep the PTE writes visible to both the compiler and remote table
    // walkers before any hart is asked to execute SFENCE.VMA.
    publish_pte_stores();
    if all {
        flush_all();
    } else {
        for &(start, size) in ranges {
            for page in (start..start + size).step_by(crate::memory::paging::PAGE_SIZE) {
                hal::paging::flush_tlb_page(page);
            }
        }
    }
    let Some((mask, base)) = crate::sched::smp::remote_online_sbi_target() else {
        return RemoteWork::None;
    };
    #[cfg(feature = "test-hooks")]
    if TEST_SKIP_REMOTE_RFENCE.load(core::sync::atomic::Ordering::Acquire) {
        return RemoteWork::None;
    }
    // A full invalidation is one RFENCE over the whole address space (SBI:
    // `size == usize::MAX` flushes everything), not a skipped RFENCE.
    let failed = if all {
        remote_sfence(mask, base, 0, usize::MAX).err()
    } else {
        ranges
            .iter()
            .find_map(|&(start, size)| remote_sfence(mask, base, start, size).err())
    };
    match failed {
        None => RemoteWork::None,
        Some(error) => {
            RFENCE_FAILURES.fetch_add(1, core::sync::atomic::Ordering::AcqRel);
            RFENCE_LAST_ERROR.store(error, core::sync::atomic::Ordering::Release);
            RemoteWork::EpochPath {
                rfence_failed: true,
            }
        }
    }
}

fn remote_sfence(mask: usize, base: usize, start: types::VAddr, size: usize) -> Result<(), usize> {
    hal::sbi::sbi_remote_sfence_vma(mask, base, start, size)
}

/// Log RFENCE failures since the last report. The flush itself cannot log: it
/// may run under `KERNEL_ROOT`, which ranks above the console. Called from the
/// hart-0 reaper, which holds no lock.
pub(crate) fn report_rfence_failures() {
    use core::sync::atomic::Ordering::{AcqRel, Acquire};
    let seen = RFENCE_FAILURES.load(Acquire);
    let reported = RFENCE_REPORTED.swap(seen, AcqRel);
    if seen > reported {
        log::error!(
            "[tlb] RV64 RFENCE failed {} time(s) (last SBI error {}); each fell back to the IPI epoch path",
            seen - reported,
            RFENCE_LAST_ERROR.load(Acquire) as isize
        );
    }
}
