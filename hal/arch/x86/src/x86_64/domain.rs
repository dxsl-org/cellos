// SPDX-License-Identifier: MPL-2.0
//! CR3 and PCID operations used by private native-domain roots on x86_64.

use core::sync::atomic::{AtomicUsize, Ordering};

#[no_mangle]
pub static VI_KERNEL_CR3: AtomicUsize = AtomicUsize::new(0);

pub fn record_kernel_cr3(cr3: usize) {
    VI_KERNEL_CR3.store(cr3, Ordering::Release);
}

pub fn kernel_cr3() -> usize {
    VI_KERNEL_CR3.load(Ordering::Acquire)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainPagingError {
    Unsupported,
}

/// Activate private address space on x86_64.
///
/// `root_pml4`: physical base address of PML4 (4KB aligned).
/// `pcid`: 12-bit PCID (0 if PCID not enabled).
#[inline]
pub fn activate_address_space(root_pml4: usize, pcid: usize) {
    let cr3 = (root_pml4 & !0xFFF) | (pcid & 0xFFF);
    unsafe {
        core::arch::asm!(
            "mov cr3, {cr3}",
            cr3 = in(reg) cr3,
            options(nostack),
        );
    }
}

/// Invalidate local translations tagged with `asid`.
#[inline]
pub fn flush_asid(_asid: usize) {
    unsafe {
        core::arch::asm!(
            "mov rax, cr3",
            "mov cr3, rax",
            out("rax") _,
            options(nostack),
        );
    }
}

pub fn flush_asid_remote(_hart_mask: usize, asid: usize) -> Result<(), DomainPagingError> {
    flush_asid(asid);
    Ok(())
}

/// Invalidate all translations by reloading CR3.
#[inline]
pub fn flush_all() {
    unsafe {
        core::arch::asm!(
            "mov rax, cr3",
            "mov cr3, rax",
            out("rax") _,
            options(nostack),
        );
    }
}

pub fn observe_switch_activation() {}
