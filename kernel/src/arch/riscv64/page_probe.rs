// SPDX-License-Identifier: MPL-2.0
//! `page_probe`: the raw translation-root register.
//!
//! The neutral reads, `leaf(root, va)` and `current_root()`, live in the HAL
//! (`hal::paging`).

/// The raw `satp` CSR.
#[inline(always)]
pub(crate) fn root_register() -> usize {
    let satp: usize;
    // SAFETY: `satp` CSR holds the page-table root PPN; reading it has no side effect.
    unsafe { core::arch::asm!("csrr {}, satp", out(reg) satp) };
    satp
}
