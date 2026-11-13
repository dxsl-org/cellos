// SPDX-License-Identifier: MPL-2.0
//! `smp`: per-hart registers.

/// The `tp` register (this hart's `ViHartLocal`, or 0 before install).
#[inline(always)]
pub(crate) fn read_tp() -> usize {
    let tp: usize;
    // SAFETY: reading `tp` has no side effects.
    unsafe {
        core::arch::asm!("mv {}, tp", out(reg) tp, options(nomem, nostack, preserves_flags));
    }
    tp
}

/// Write `tp`.
///
/// # Safety
/// `val` is a valid `HART_LOCALS` pointer or 0, and no concurrent trap may
/// read a partial update (see `hart_local::write_tp`).
#[inline(always)]
pub(crate) unsafe fn write_tp(val: usize) {
    // SAFETY: writing tp CSR is always safe from S-mode; the value is either
    // a valid HART_LOCALS pointer or 0 (pre-install). Caller ensures context.
    core::arch::asm!("mv tp, {}", in(reg) val, options(nomem, nostack, preserves_flags));
}
