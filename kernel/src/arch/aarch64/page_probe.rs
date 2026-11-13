// SPDX-License-Identifier: MPL-2.0
//! `page_probe`: read the live translation root and walk one root to a leaf.
//!
//! `leaf` and `current_root` report the neutral `SV39_*` leaf bits of
//! `task::user_copy::sv39_probe`; `root_register` is the raw root register.

#[cfg(feature = "native-domains")]
use crate::task::user_copy::sv39_probe::{SV39_READ, SV39_USER, SV39_VALID, SV39_WRITE};

/// Walk one Sv39 root and return `(PTE flag bits, resolved physical address)`
/// for the leaf translating `va`. Reads page-table memory through the kernel
/// linear mapping, so the walk itself can never take a user page fault.
#[cfg(feature = "native-domains")]
pub(crate) fn leaf(root_pa: usize, va: usize) -> Option<(usize, usize)> {
    let table = unsafe { &*(crate::memory::frame::phys_to_virt(root_pa) as *const hal::PageTable) };
    let leaf = table.leaf_entry(va)?;
    let pa = (leaf as usize) & 0x0000_ffff_ffff_f000;
    let mut bits = 0usize;
    if leaf & 1 != 0 {
        bits |= SV39_VALID;
    }
    if leaf & (1 << 6) != 0 {
        bits |= SV39_USER;
    }
    bits |= SV39_READ;
    if leaf & (1 << 7) == 0 {
        bits |= SV39_WRITE;
    }
    Some((bits, pa))
}

/// Physical root of the currently resident domain table, if paging is active.
#[cfg(feature = "native-domains")]
pub(crate) fn current_root() -> Option<usize> {
    let ttbr0: usize;
    unsafe {
        core::arch::asm!(
            "mrs {ttbr0}, ttbr0_el1",
            ttbr0 = out(reg) ttbr0,
            options(nostack, nomem)
        );
    }
    Some(ttbr0 & 0x0000_ffff_ffff_f000)
}

/// The raw `TTBR0_EL1` register.
#[inline(always)]
pub(crate) fn root_register() -> usize {
    let ttbr0: usize;
    // SAFETY: TTBR0_EL1 holds the root page table physical address; reading it has no side effect.
    unsafe { core::arch::asm!("mrs {}, ttbr0_el1", out(reg) ttbr0, options(nomem, nostack)) };
    ttbr0
}
