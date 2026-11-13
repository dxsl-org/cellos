// SPDX-License-Identifier: MPL-2.0
//! `page_probe`: read the live translation root and walk one root to a leaf.
//!
//! `leaf` and `current_root` report the neutral `SV39_*` leaf bits of
//! `task::user_copy::sv39_probe`; `root_register` is the raw root register.

#[cfg(feature = "native-domains")]
use crate::task::user_copy::sv39_probe::{SV39_READ, SV39_VALID, SV39_WRITE};

/// Walk one Sv39 root and return `(PTE flag bits, resolved physical address)`
/// for the leaf translating `va`. Reads page-table memory through the kernel
/// linear mapping, so the walk itself can never take a user page fault.
#[cfg(feature = "native-domains")]
pub(crate) fn leaf(root_pa: usize, va: usize) -> Option<(usize, usize)> {
    let mut table_pa = root_pa;
    for level in 0..3usize {
        let index = (va >> (30 - 9 * level)) & 0x1FF;
        let pte = unsafe {
            core::ptr::read_volatile(
                crate::memory::frame::phys_to_virt(table_pa + index * 8) as *const u64
            )
        } as usize;
        if pte & SV39_VALID == 0 {
            return None;
        }
        if pte & (SV39_READ | SV39_WRITE | (1 << 3)) != 0 {
            let shift = 12 + 9 * (2 - level);
            let off_mask = (1usize << shift) - 1;
            let pa = (((pte >> 10) & ((1usize << 44) - 1)) << 12) | (va & off_mask);
            return Some((pte & 0xFF, pa));
        }
        table_pa = ((pte >> 10) & ((1usize << 44) - 1)) << 12;
    }
    None
}

/// Physical root of the currently resident domain table, if paging is active.
#[cfg(feature = "native-domains")]
pub(crate) fn current_root() -> Option<usize> {
    let satp: usize;
    unsafe {
        core::arch::asm!(
            "csrr {satp}, satp",
            satp = out(reg) satp,
            options(nostack, nomem)
        );
    }
    if (satp >> 60) != 8 {
        return None;
    }
    Some((satp & ((1usize << 44) - 1)) << 12)
}

/// The raw `satp` CSR.
#[inline(always)]
pub(crate) fn root_register() -> usize {
    let satp: usize;
    // SAFETY: `satp` CSR holds the page-table root PPN; reading it has no side effect.
    unsafe { core::arch::asm!("csrr {}, satp", out(reg) satp) };
    satp
}
