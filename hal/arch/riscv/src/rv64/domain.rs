//! SATP and ASID operations used exclusively by private native-domain roots.
//!
//! The scheduler must not write SATP directly. These operations keep the PTE-store,
//! SATP, and `sfence.vma` ordering at the architecture boundary.

#[cfg(feature = "test-hooks")]
use core::sync::atomic::{AtomicUsize, Ordering};

#[cfg(feature = "test-hooks")]
static SATP_WRITES: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "test-hooks")]
static ASID_FLUSHES: AtomicUsize = AtomicUsize::new(0);

/// Test-only observation of scheduler domain activation, never an admission signal.
#[cfg(feature = "test-hooks")]
pub fn switch_counters() -> (usize, usize) {
    (
        SATP_WRITES.load(Ordering::Acquire),
        ASID_FLUSHES.load(Ordering::Acquire),
    )
}

#[cfg(feature = "test-hooks")]
pub fn reset_switch_counters() {
    SATP_WRITES.store(0, Ordering::Release);
    ASID_FLUSHES.store(0, Ordering::Release);
}

/// Records a scheduler-proven SATP + ASID fence pair for test fixtures.
#[inline]
pub fn observe_switch_activation() {
    #[cfg(feature = "test-hooks")]
    {
        SATP_WRITES.fetch_add(1, Ordering::Relaxed);
        ASID_FLUSHES.fetch_add(1, Ordering::Relaxed);
    }
}
use crate::common::sbi;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainPagingError {
    Firmware(usize),
}

/// Install an Sv39 root and invalidate stale translations for its ASID.
///
/// `root_ppn` is a physical page number, not a byte address.
#[inline]
pub fn activate_address_space(root_ppn: usize, asid: usize) {
    let satp = (8usize << 60) | ((asid & 0xffff) << 44) | (root_ppn & ((1usize << 44) - 1));
    // SAFETY: this is called only after the private root and its supervisor
    // entry pages have been completed; the fence orders every preceding PTE store.
    unsafe {
        core::arch::asm!(
            "fence rw, rw",
            "csrw satp, {satp}",
            "sfence.vma zero, {asid}",
            satp = in(reg) satp,
            asid = in(reg) asid,
            options(nostack),
        );
    }
    #[cfg(feature = "test-hooks")]
    {
        SATP_WRITES.fetch_add(1, Ordering::Relaxed);
        ASID_FLUSHES.fetch_add(1, Ordering::Relaxed);
    }
}

/// Invalidate local translations tagged with `asid`.
#[inline]
pub fn flush_asid(asid: usize) {
    // SAFETY: `sfence.vma` is valid in S-mode and does not access Rust memory.
    unsafe {
        core::arch::asm!("sfence.vma zero, {asid}", asid = in(reg) asid, options(nostack));
    }
    #[cfg(feature = "test-hooks")]
    ASID_FLUSHES.fetch_add(1, Ordering::Relaxed);
}

/// Invalidate a selected hart set before its ASID can be recycled.
///
/// SBI RFENCE has no single-ASID call in the deployed firmware baseline, so this
/// deliberately performs the stronger all-ASID remote invalidation.
pub fn flush_asid_remote(hart_mask: usize, _asid: usize) -> Result<(), DomainPagingError> {
    if hart_mask == 0 {
        return Ok(());
    }
    sbi::sbi_remote_sfence_vma(hart_mask, 0, 0, usize::MAX).map_err(DomainPagingError::Firmware)
}

/// Invalidate every local translation before an ASID epoch wraps.
#[inline]
pub fn flush_all() {
    // SAFETY: see `flush_asid`; x0 selects every virtual address and ASID.
    unsafe {
        core::arch::asm!("sfence.vma zero, zero", options(nostack));
    }
}
