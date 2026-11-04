//! Model-specific register access for code added after the per-module
//! helpers (`cet.rs`, `svm.rs`, `syscall.rs`, `vmx.rs`), which keep their own.

/// Read MSR `msr`.
///
/// # Safety
/// Ring 0 only, and `msr` must exist on this CPU (otherwise `#GP`).
#[inline]
pub unsafe fn read(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    // SAFETY: the caller guarantees ring 0 and an implemented MSR.
    unsafe {
        core::arch::asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi,
            options(nomem, nostack, preserves_flags));
    }
    (hi as u64) << 32 | lo as u64
}

/// Write `value` to MSR `msr`.
///
/// # Safety
/// Ring 0 only; `msr` must exist and accept `value` (otherwise `#GP`), and the
/// write's side effect must be valid for the caller's state.
#[inline]
pub unsafe fn write(msr: u32, value: u64) {
    // SAFETY: the caller guarantees ring 0, an implemented MSR and a valid value.
    unsafe {
        core::arch::asm!("wrmsr", in("ecx") msr, in("eax") value as u32,
            in("edx") (value >> 32) as u32, options(nostack, preserves_flags));
    }
}
