//! ARM Generic Timer driver.
//!
//! EL1 (default):  CNTP_TVAL_EL0 / CNTP_CTL_EL0.
//! EL2 host:       CNTHP_TVAL_EL2 / CNTHP_CTL_EL2.
//!
//! Runtime dispatch via `el2::is_el2()` (set before `kmain` in boot.rs).
//!
//! On QEMU virt: CNTFRQ_EL0 = 62.5 MHz, TICKS_PER_QUANTUM = 625_000.
//! On RPi 3 (BCM2837): CNTFRQ_EL0 = 19.2 MHz, TICKS_PER_QUANTUM = 192_000.
//! `ticks_per_quantum()` reads CNTFRQ_EL0 at runtime to handle both.
//!
//! The host tick source is the board backend's choice ([`ArmTimer`]);
//! [`init_gic`] and [`reset_generic`] are the generic-timer tick shared by the
//! GICv2 backends.

use super::backend::{ArmTimer, Board};
use super::gic::{self, GicV2Layout};

/// Fallback ticks-per-quantum for QEMU virt (~10 ms @ 62.5 MHz).
const TICKS_PER_QUANTUM_QEMU: u64 = 625_000;

/// The system counter (`CNTPCT_EL0`), in its own units.
///
/// Used by bounded probes that must not wait on an interrupt — it advances
/// whether or not IRQs are unmasked in the caller's context. `CNTFRQ_EL0`
/// gives the units-per-second.
pub fn counter_now() -> u64 {
    let value: u64;
    // SAFETY: CNTPCT_EL0 is readable from EL1 and modifies nothing.
    unsafe {
        core::arch::asm!("mrs {}, cntpct_el0", out(reg) value, options(nomem, nostack));
    }
    value
}

/// The virtual counter (`CNTVCT_EL0` = `CNTPCT_EL0` − `CNTVOFF_EL2`): the count
/// a guest's virtual timer compares `CNTV_CVAL` against, read with the
/// current virtual offset.
pub fn virtual_count_now() -> u64 {
    let value: u64;
    // SAFETY: CNTVCT_EL0 is readable from EL1/EL2 and modifies nothing.
    unsafe {
        core::arch::asm!("mrs {}, cntvct_el0", out(reg) value, options(nomem, nostack));
    }
    value
}

/// The system counter's frequency in Hz (`CNTFRQ_EL0`).
///
/// Absent firmware that reports a frequency (0) falls back to QEMU virt's
/// 62.5 MHz: the callers turn this into a *bounded* budget, never an exact
/// interval, so a wrong frequency shortens or lengthens a probe rather than
/// changing what is correct.
pub fn counter_frequency_hz() -> u64 {
    let freq: u64;
    // SAFETY: CNTFRQ_EL0 is a read-only system register at every exception level.
    unsafe {
        core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nomem, nostack));
    }
    if freq == 0 {
        62_500_000
    } else {
        freq
    }
}

/// Compute 10 ms quantum by reading the actual timer frequency from CNTFRQ_EL0.
pub fn ticks_per_quantum() -> u64 {
    let freq: u64;
    // SAFETY: CNTFRQ_EL0 is read-only from EL1/EL2; no state modified.
    unsafe {
        core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nomem, nostack));
    }
    if freq == 0 {
        return TICKS_PER_QUANTUM_QEMU;
    }
    freq / 100 // 10 ms = 1/100 s
}

/// Initialise and arm the board's host tick.
pub fn init() {
    <Board as ArmTimer>::init();
}

/// Arm the ARM generic timer as the host tick, with its PPI enabled at the GIC.
///
/// Uses the EL2 hypervisor physical timer (CNTHP) when `el2::is_el2()` is
/// true, otherwise the EL1 physical timer (CNTP).
pub fn init_gic<G: GicV2Layout>() {
    let ticks = ticks_per_quantum();
    if super::el2::is_el2() {
        // SAFETY: CNTHP_* are EL2-private hypervisor physical timer registers.
        unsafe {
            core::arch::asm!(
                "msr cnthp_tval_el2, {val}",
                "mov {ctl}, #1",
                "msr cnthp_ctl_el2,  {ctl}",
                val = in(reg) ticks,
                ctl = out(reg) _,
                options(nomem, nostack),
            );
        }
        gic::enable_irq::<G>(26);
    } else {
        // SAFETY: CNTP_* system registers; EL1-private.
        unsafe {
            core::arch::asm!(
                "msr cntp_tval_el0, {val}",
                "mov {ctl}, #1",
                "msr cntp_ctl_el0,  {ctl}",
                val = in(reg) ticks,
                ctl = out(reg) _,
                options(nomem, nostack),
            );
        }
        gic::enable_irq::<G>(30);
    }
}

/// Re-arm the board's host tick for the next quantum.  Call from the IRQ handler.
pub fn reset() {
    <Board as ArmTimer>::reset();
}

/// Reload the ARM generic timer: CNTHP_TVAL_EL2 (EL2) or CNTP_TVAL_EL0 (EL1).
pub fn reset_generic() {
    let ticks = ticks_per_quantum();
    if super::el2::is_el2() {
        // SAFETY: same as init() EL2 branch.
        unsafe {
            core::arch::asm!(
                "msr cnthp_tval_el2, {val}",
                val = in(reg) ticks,
                options(nomem, nostack),
            );
        }
    } else {
        // SAFETY: same as init() EL1 branch.
        unsafe {
            core::arch::asm!(
                "msr cntp_tval_el0, {val}",
                val = in(reg) ticks,
                options(nomem, nostack),
            );
        }
    }
}

/// Read the current cycle counter (CNTPCT_EL0).
pub fn read_ticks() -> u64 {
    let val: u64;
    // SAFETY: CNTPCT_EL0 is read-only; no state modified.
    unsafe {
        core::arch::asm!("mrs {}, cntpct_el0", out(reg) val, options(nomem, nostack));
    }
    val
}

/// Read the timer frequency (CNTFRQ_EL0) in Hz.
pub fn read_freq() -> u64 {
    let val: u64;
    // SAFETY: CNTFRQ_EL0 is read-only; no state modified.
    unsafe {
        core::arch::asm!("mrs {}, cntfrq_el0", out(reg) val, options(nomem, nostack));
    }
    val
}
