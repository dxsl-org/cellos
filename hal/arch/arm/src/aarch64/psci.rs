//! PSCI client — `CPU_ON` for secondary cores.
//!
//! Only the SMC conduit and only `PSCI_VERSION`/`PSCI_CPU_ON` are implemented:
//! this kernel starts parked secondaries and never suspends a core. The conduit
//! is *declared by firmware* (`/psci/method` in the device tree) and read once
//! at boot — an SMC on a machine with no EL3 monitor is an undefined
//! instruction, so nothing here issues a call the tree did not ask for.
//!
//! Callers must be at EL1 with the kernel's exception vectors installed: a PSCI
//! call that firmware refuses returns a status, it never returns a value.

use core::arch::asm;

/// PSCI `SUCCESS`.
pub const SUCCESS: i64 = 0;

/// `PSCI_VERSION` (SMCCC 64-bit function ID).
const FN_VERSION: u64 = 0x8400_0000;
/// `PSCI_CPU_ON` (SMCCC 64-bit function ID).
const FN_CPU_ON: u64 = 0xC400_0003;

/// Which instruction the firmware expects for PSCI calls.
///
/// A guest at EL1 calls EL3 firmware (`smc`); a guest at EL2 — this kernel's
/// development Silo, and any guest under a hypervisor — calls its host (`hvc`).
/// The machine's device tree states which (`/psci/method`), and firmware that
/// answers the wrong one traps as an undefined instruction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Conduit {
    /// `/psci/method = "smc"`.
    Smc,
    /// `/psci/method = "hvc"`.
    Hvc,
}

/// Issue one SMCCC call over `conduit`.
///
/// x4–x17 are volatile under SMCCC (the firmware may clobber them), so they are
/// declared as clobbers: a caller holding a live value in one of them would
/// otherwise see it silently destroyed across the call.
fn call(conduit: Conduit, function: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    let mut x0 = function;
    // SAFETY: `smc #0` is the SMCCC conduit; the kernel only calls it from EL1
    // after the firmware tree declared this conduit. `nostack` is sound: the
    // instruction touches no kernel memory and returns to the next instruction.
    unsafe {
        macro_rules! smccc {
            ($instruction:literal) => {
                asm!(
                    $instruction,
                    inout("x0") x0,
                    // x1–x3 are arguments to the firmware and volatile under
                    // SMCCC: the results are not read back, only clobbered.
                    inout("x1") arg1 => _,
                    inout("x2") arg2 => _,
                    inout("x3") arg3 => _,
                    out("x4") _, out("x5") _, out("x6") _, out("x7") _,
                    out("x8") _, out("x9") _, out("x10") _, out("x11") _,
                    out("x12") _, out("x13") _, out("x14") _, out("x15") _,
                    out("x16") _, out("x17") _,
                    options(nostack)
                )
            };
        }
        match conduit {
            Conduit::Smc => smccc!("smc #0"),
            Conduit::Hvc => smccc!("hvc #0"),
        }
    }
    x0 as i64
}

/// Does this CPU implement EL3?
///
/// With EL3 the PSCI conduit is EL3 firmware's (`smc`); without it an SMC is
/// architecturally undefined, and the only conduit that can exist is the
/// hypervisor's (`hvc`). Read from `ID_AA64PFR0_EL1.EL3`, so the inference is a
/// register fact rather than a guess about the machine.
pub fn el3_implemented() -> bool {
    let pfr0: u64;
    // SAFETY: ID_AA64PFR0_EL1 is readable from EL1/EL2 and modifies nothing.
    unsafe {
        core::arch::asm!("mrs {}, id_aa64pfr0_el1", out(reg) pfr0, options(nomem, nostack));
    }
    ((pfr0 >> 12) & 0xF) != 0
}

/// The conduit to use when no firmware tree declares one.
pub fn inferred_conduit() -> Conduit {
    if el3_implemented() {
        Conduit::Smc
    } else {
        Conduit::Hvc
    }
}

/// `PSCI_VERSION`, or `None` when the conduit does not implement PSCI.
///
/// Used as the presence probe: a firmware that answers with a version is one
/// that will also answer `CPU_ON`.
pub fn version(conduit: Conduit) -> Option<u32> {
    let raw = call(conduit, FN_VERSION, 0, 0, 0);
    (raw >= 0).then_some(raw as u32)
}

/// `PSCI_CPU_ON`: start the core identified by `mpidr` at `entry` (physical
/// address, identity-mapped) with `context` delivered in x0.
pub fn cpu_on(conduit: Conduit, mpidr: u64, entry: usize, context: u64) -> Result<(), i64> {
    match call(conduit, FN_CPU_ON, mpidr, entry as u64, context) {
        SUCCESS => Ok(()),
        status => Err(status),
    }
}

/// PSCI status name, for the kernel log.
pub fn status_name(status: i64) -> &'static str {
    match status {
        -1 => "NOT_SUPPORTED",
        -2 => "INVALID_PARAMETERS",
        -3 => "DENIED",
        -4 => "ALREADY_ON",
        -5 => "ON_PENDING",
        -6 => "INTERNAL_FAILURE",
        -7 => "NOT_PRESENT",
        -8 => "DISABLED",
        _ => "UNKNOWN",
    }
}
