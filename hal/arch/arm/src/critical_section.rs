//! critical-section implementation for AArch64 (EL1/EL2).
//!
//! Enabled via the `critical-section-impl` feature on `hal-arm`.
//! Only one impl may be linked per binary — the kernel enables this feature;
//! Cell crates must NOT enable it.
//!
//! Mechanism: `mrs`/`msr daif` saves and restores the full DAIF register.
//! `acquire` sets DAIF.I (bit 7, mask = #2) to block IRQs.
//! `release` restores the saved DAIF, re-enabling IRQs only if they were
//! unmasked before `acquire`.
//!
//! AArch32 targets have no impl here — they require a separate CPSR-based
//! implementation that is not in scope for ViCell.
//!
//! # SMP limitation
//!
//! This impl masks IRQs on the **local PE only**. On multi-core ViCell builds,
//! code inside `critical_section::with` is NOT mutually exclusive against
//! concurrent execution on other PEs. Use a `Spinlock` for cross-core exclusion.

#[cfg(target_arch = "aarch64")]
mod aarch64_impl {
    use critical_section::{set_impl, Impl, RawRestoreState};

    struct ViArm64Cs;
    set_impl!(ViArm64Cs);

    unsafe impl Impl for ViArm64Cs {
        unsafe fn acquire() -> RawRestoreState {
            let daif: usize;
            // SAFETY: MRS DAIF is always permitted at EL1/EL2.
            // MSR DAIFSet sets the I bit (IRQ mask) without affecting other bits.
            // We save the full DAIF so release() restores the exact prior state.
            // The mrs-then-daifset sequence is not atomic: an IRQ can arrive
            // between the two instructions. This is benign — the IRQ is handled
            // and returns before daifset executes, and acquire() records the
            // pre-IRQ DAIF correctly, so nesting is sound.
            core::arch::asm!(
                "mrs {daif}, daif",
                "msr daifset, #2",
                daif = out(reg) daif,
                // nostack: no stack ops. nomem absent: compiler barrier for ordering.
                options(nostack),
            );
            daif
        }

        unsafe fn release(daif: RawRestoreState) {
            // SAFETY: MSR DAIF is permitted at EL1/EL2.
            // Restores DAIF to the value saved by acquire().
            core::arch::asm!(
                "msr daif, {daif}",
                daif = in(reg) daif,
                options(nostack),
            );
        }
    }
}
