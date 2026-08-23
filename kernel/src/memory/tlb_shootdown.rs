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

        hal::paging::flush_tlb_page(start);
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
    {
        let _ = size;
        hal::paging::flush_tlb_page(start);
    }

    #[cfg(target_arch = "riscv32")]
    {
        // RV32 boards use the bare-physical SATP=0 contract and have no TLB.
        let _ = (start, size);
    }
}
