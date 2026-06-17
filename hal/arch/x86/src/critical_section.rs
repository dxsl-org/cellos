//! critical-section implementation for x86_64 (ring 0).
//!
//! Enabled via the `critical-section-impl` feature on `hal-x86`.
//! Only one impl may be linked per binary — the kernel enables this feature;
//! Cell crates must NOT enable it.
//!
//! Mechanism: `pushfq`/`pop` saves RFLAGS; `cli` clears IF (interrupt flag).
//! `release` pushes the saved RFLAGS and `popfq` restores it, re-enabling
//! interrupts only if they were enabled before `acquire`.
//!
//! # SMP limitation
//!
//! `cli`/`sti` only affect the **local CPU**. On multi-core builds, code inside
//! `critical_section::with` is NOT mutually exclusive against other CPUs.
//! Use a spinlock for cross-CPU exclusion.

#[cfg(target_arch = "x86_64")]
mod x86_64_impl {
    use critical_section::{set_impl, Impl, RawRestoreState};

    struct ViX86Cs;
    set_impl!(ViX86Cs);

    unsafe impl Impl for ViX86Cs {
        unsafe fn acquire() -> RawRestoreState {
            let rflags: usize;
            // SAFETY: pushfq/pop and cli are valid in ring 0.
            // pushfq/pop reads RFLAGS without modifying it, then cli clears IF.
            core::arch::asm!(
                "pushfq",
                "pop {rflags}",
                "cli",
                rflags = out(reg) rflags,
            );
            rflags
        }

        unsafe fn release(rflags: RawRestoreState) {
            // SAFETY: push/popfq is valid in ring 0.
            // Restores RFLAGS (including IF) to the value saved by acquire().
            core::arch::asm!(
                "push {rflags}",
                "popfq",
                rflags = in(reg) rflags,
            );
        }
    }
}
