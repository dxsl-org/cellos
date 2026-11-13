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
    let mut table_pa = root_pa;
    for level in 0..4usize {
        let shift = 39 - 9 * level;
        let index = (va >> shift) & 0x1FF;
        let pte = unsafe {
            core::ptr::read_volatile(
                crate::memory::frame::phys_to_virt(table_pa + index * 8) as *const u64
            )
        } as usize;
        if pte & 1 == 0 {
            return None;
        }
        if level == 3 || pte & (1 << 7) != 0 {
            let pa = pte & 0x000f_ffff_ffff_f000;
            let mut bits = SV39_VALID | SV39_READ;
            if pte & (1 << 1) != 0 {
                bits |= SV39_WRITE;
            }
            if pte & (1 << 2) != 0 {
                bits |= SV39_USER;
            }
            return Some((bits, pa));
        }
        table_pa = pte & 0x000f_ffff_ffff_f000;
    }
    None
}

/// Physical root of the currently resident domain table, if paging is active.
#[cfg(feature = "native-domains")]
pub(crate) fn current_root() -> Option<usize> {
    let cr3: usize;
    unsafe {
        core::arch::asm!(
            "mov {cr3}, cr3",
            cr3 = out(reg) cr3,
            options(nostack, nomem)
        );
    }
    Some(cr3 & !0xFFF)
}

/// The raw `CR3` register.
#[cfg(feature = "p01-trace")]
#[inline(always)]
pub(crate) fn root_register() -> usize {
    let cr3: usize;
    // SAFETY: reading CR3 has no side effect.
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    }
    cr3
}
