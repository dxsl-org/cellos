//! Pi EL1-host vCPU extension: the eager Q0–Q31/FPCR/FPSR bank switch that
//! keeps guest FP and Cell FP isolated across every EL2 transition, and the
//! EL1 sysregs the Pi world switch carries.

use crate::aarch64::backend::ArmVcpuExt;

/// State appended to `AArch64Vcpu` on the Pi. The world-switch assembly in
/// `vcpu.rs` addresses the FP fields by absolute offset (see the assertions).
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct PiVcpuExt {
    pub g_fp_regs: [u128; 32],
    pub g_fpcr: u64,
    pub g_fpsr: u64,
    pub h_fp_regs: [u128; 32],
    pub h_fpcr: u64,
    pub h_fpsr: u64,
    pub g_cpacr_el1: u64,
    pub g_cntkctl_el1: u64,
    pub g_tpidrro_el0: u64,
}

/// Host EL1 sysregs the Pi world switch saves around a guest run.
#[derive(Default)]
pub struct PiHostBank {
    cpacr_el1: u64,
    cntkctl_el1: u64,
    tpidrro_el0: u64,
}

// The world-switch assembly's absolute offsets: FP bank at 544, FPCR 1056,
// host bank 1072, host FPCR 1584 (extension at 544, struct size 1632).
const _: () = {
    const BASE: usize = <PiVcpuExt as ArmVcpuExt>::VCPU_EXT_OFFSET;
    assert!(BASE + core::mem::offset_of!(PiVcpuExt, g_fp_regs) == 544);
    assert!(BASE + core::mem::offset_of!(PiVcpuExt, g_fpcr) == 1056);
    assert!(BASE + core::mem::offset_of!(PiVcpuExt, h_fp_regs) == 1072);
    assert!(BASE + core::mem::offset_of!(PiVcpuExt, h_fpcr) == 1584);
};

impl ArmVcpuExt for PiVcpuExt {
    type HostBank = PiHostBank;

    const PI_WORLD_SWITCH: bool = true;
    /// Mode 2 reads the guest virtual-timer pair (Pi only).
    const REGS_MAX_MODE: usize = 2;
    const VCPU_SIZE: usize = 1632;
    const VCPU_EXT_OFFSET: usize = 544;

    #[inline(always)]
    fn save_host(bank: &mut PiHostBank) {
        // SAFETY: reading EL1/EL0 sysregs from EL2 is unconditionally permitted.
        unsafe {
            core::arch::asm!(
                "mrs {cpacr}, cpacr_el1",
                "mrs {cntkctl}, cntkctl_el1",
                "mrs {tpidrro}, tpidrro_el0",
                cpacr = out(reg) bank.cpacr_el1,
                cntkctl = out(reg) bank.cntkctl_el1,
                tpidrro = out(reg) bank.tpidrro_el0,
                options(nomem, nostack),
            );
        }
    }

    #[inline(always)]
    fn restore_guest(&self) {
        // SAFETY: loading the guest's EL1 bank from EL2 before guest entry.
        unsafe {
            core::arch::asm!(
                "msr cpacr_el1, {cpacr}",
                "msr cntkctl_el1, {cntkctl}",
                "msr tpidrro_el0, {tpidrro}",
                cpacr = in(reg) self.g_cpacr_el1,
                cntkctl = in(reg) self.g_cntkctl_el1,
                tpidrro = in(reg) self.g_tpidrro_el0,
                options(nomem, nostack),
            );
        }
    }

    #[inline(always)]
    fn save_guest(&mut self) {
        // SAFETY: the guest is stopped; reading EL1 sysregs from EL2 is permitted.
        unsafe {
            core::arch::asm!(
                "mrs {cpacr}, cpacr_el1",
                "mrs {cntkctl}, cntkctl_el1",
                "mrs {tpidrro}, tpidrro_el0",
                cpacr = out(reg) self.g_cpacr_el1,
                cntkctl = out(reg) self.g_cntkctl_el1,
                tpidrro = out(reg) self.g_tpidrro_el0,
                options(nomem, nostack),
            );
        }
    }

    #[inline(always)]
    fn restore_host(bank: &PiHostBank) {
        // SAFETY: restoring our own host state; EL1 sysregs safe to write from EL2.
        unsafe {
            core::arch::asm!(
                "msr cpacr_el1, {cpacr}",
                "msr cntkctl_el1, {cntkctl}",
                "msr tpidrro_el0, {tpidrro}",
                cpacr = in(reg) bank.cpacr_el1,
                cntkctl = in(reg) bank.cntkctl_el1,
                tpidrro = in(reg) bank.tpidrro_el0,
                options(nomem, nostack),
            );
        }
    }
}
