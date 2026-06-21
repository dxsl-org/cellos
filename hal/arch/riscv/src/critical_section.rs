//! critical-section implementation for RISC-V (S-mode, both rv32 and rv64).
//!
//! Enabled via the `critical-section-impl` feature on `hal-riscv`.
//! Only one impl may be linked per binary — the kernel enables this feature;
//! Cell crates must NOT enable it.
//!
//! Mechanism: `csrrci` atomically reads and clears sstatus.SIE (bit 1).
//! `release` restores the full saved sstatus, re-enabling interrupts only if
//! they were enabled before `acquire`.
//!
//! # SMP limitation
//!
//! This impl masks interrupts on the **local hart only**. ViCell supports SMP
//! (MAX_HARTS=2, hart 1 = RT scheduler). Code protected by
//! `critical_section::with` is therefore NOT mutually exclusive against
//! concurrent execution on hart 1.
//!
//! **Safe to use:** `heapless::spsc::Queue` (single-producer, single-consumer),
//! any data only ever accessed from one hart, and interrupt-handler ↔ task
//! synchronisation on the same hart.
//!
//! **Unsafe to use:** `heapless::mpmc::Queue` or any structure that requires
//! exclusion across all harts simultaneously. Use a `Spinlock` for those.

use critical_section::{set_impl, Impl, RawRestoreState};

struct ViRiscvCs;
set_impl!(ViRiscvCs);

unsafe impl Impl for ViRiscvCs {
    unsafe fn acquire() -> RawRestoreState {
        let sstatus: usize;
        // SAFETY: csrrci is an atomic read-modify-write on sstatus from S-mode.
        // Returns the old sstatus so release() can restore it exactly.
        // Bit 1 = SIE (Supervisor Interrupt Enable).
        core::arch::asm!(
            "csrrci {}, sstatus, 0x2",
            out(reg) sstatus,
            // nostack: no stack ops. nomem intentionally absent: the asm acts as a
            // compiler barrier so memory ops do not migrate across acquire/release.
            options(nostack),
        );
        sstatus
    }

    unsafe fn release(sstatus: RawRestoreState) {
        // SAFETY: Restoring sstatus to the saved value from acquire().
        // If SIE was set before acquire, this re-enables interrupts.
        core::arch::asm!(
            "csrw sstatus, {}",
            in(reg) sstatus,
            options(nostack),
        );
    }
}
