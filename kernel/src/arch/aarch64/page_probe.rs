// SPDX-License-Identifier: MPL-2.0
//! `page_probe`: the raw translation-root register.
//!
//! The neutral reads, `leaf(root, va)` and `current_root()`, live in the HAL
//! (`hal::paging`).

/// The raw `TTBR0_EL1` register.
#[inline(always)]
pub(crate) fn root_register() -> usize {
    let ttbr0: usize;
    // SAFETY: TTBR0_EL1 holds the root page table physical address; reading it has no side effect.
    unsafe { core::arch::asm!("mrs {}, ttbr0_el1", out(reg) ttbr0, options(nomem, nostack)) };
    ttbr0
}
