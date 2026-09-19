// SPDX-License-Identifier: MPL-2.0
//! TTBR0_EL1 and ASID operations used by private native-domain roots on AArch64.

use core::sync::atomic::{AtomicUsize, Ordering};

pub static VI_KERNEL_TTBR0: AtomicUsize = AtomicUsize::new(0);

pub fn record_kernel_ttbr0(ttbr0: usize) {
    VI_KERNEL_TTBR0.store(ttbr0, Ordering::Release);
}

pub fn kernel_ttbr0() -> usize {
    VI_KERNEL_TTBR0.load(Ordering::Acquire)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainPagingError {
    Unsupported,
}

/// Activate private address space on AArch64.
///
/// `root_baddr`: physical base address of L1 page table (4KB aligned).
/// `asid`: 16-bit address space ID.
#[inline]
pub fn activate_address_space(root_baddr: usize, asid: usize) {
    let ttbr0 = ((asid as u64 & 0xffff) << 48) | (root_baddr as u64 & 0x0000_ffff_ffff_f000);
    unsafe {
        core::arch::asm!(
            "dsb ishst",
            "msr ttbr0_el1, {ttbr0}",
            "isb",
            ttbr0 = in(reg) ttbr0,
            options(nostack),
        );
    }
}

/// Invalidate local translations tagged with `asid`.
#[inline]
pub fn flush_asid(asid: usize) {
    let asid_val = (asid as u64 & 0xffff) << 48;
    unsafe {
        core::arch::asm!(
            "dsb ishst",
            "tlbi aside1is, {asid}",
            "dsb ish",
            "isb",
            asid = in(reg) asid_val,
            options(nostack),
        );
    }
}

pub fn flush_asid_remote(_hart_mask: usize, asid: usize) -> Result<(), DomainPagingError> {
    // On AArch64, `tlbi aside1is` is already broadcast across all PEs in the Inner Shareable domain.
    flush_asid(asid);
    Ok(())
}

/// Invalidate all EL1 translations across the inner-shareable domain.
#[inline]
pub fn flush_all() {
    unsafe {
        core::arch::asm!(
            "dsb ishst",
            "tlbi vmalle1is",
            "dsb ish",
            "isb",
            options(nostack),
        );
    }
}

pub fn observe_switch_activation() {}
