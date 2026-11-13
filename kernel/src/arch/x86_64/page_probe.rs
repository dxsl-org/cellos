// SPDX-License-Identifier: MPL-2.0
//! `page_probe`: the raw translation-root register.
//!
//! The neutral reads, `leaf(root, va)` and `current_root()`, live in the HAL
//! (`hal::paging`).

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
