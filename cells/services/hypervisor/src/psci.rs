//! PSCI (Power State Coordination Interface) HVC emulator.
//!
//! Handles the small set of SMCCC function IDs that Linux arm64 calls during boot.
//! All calls arrive as `ViVmExit::Hvc` from the run loop after the guest issues `HVC #0`.
//! The PSCI method in the DTB must be "hvc" (not "smc") — otherwise Linux won't trap here.

/// PSCI function IDs (SMCCC 32/64-bit forms where applicable).
pub mod fid {
    pub const VERSION: u64 = 0x8400_0000;
    pub const CPU_SUSPEND: u64 = 0x8400_0001;
    pub const CPU_OFF: u64 = 0x8400_0002;
    pub const CPU_ON: u64 = 0x8400_0003;
    pub const AFFINITY_INFO: u64 = 0x8400_0004;
    pub const SYSTEM_OFF: u64 = 0x8400_0008;
    pub const SYSTEM_RESET: u64 = 0x8400_0009;
    pub const FEATURES: u64 = 0x8400_000A;
    // 64-bit variants (bit 30 set).
    pub const CPU_SUSPEND_64: u64 = 0xC400_0001;
    pub const CPU_ON_64: u64 = 0xC400_0003;
    pub const AFFINITY_INFO_64: u64 = 0xC400_0004;
}

/// PSCI return codes.
pub mod ret {
    pub const SUCCESS: u64 = 0;
    pub const NOT_SUPPORTED: u64 = u64::MAX; // -1 as u64
    pub const DENIED: u64 = u64::MAX - 2; // -3 as u64
}

/// Outcome of a PSCI HVC call.
pub enum PsciAction {
    /// Return `result` in x0 and continue running the guest.
    Return(u64),
    /// Guest called SYSTEM_OFF — tear down the VM and exit.
    SystemOff,
    /// Guest called SYSTEM_RESET — treat as SystemOff for now.
    SystemReset,
}

/// Dispatch a PSCI HVC call.
///
/// `regs[0]` = SMCCC function ID, `regs[1..7]` = arguments.
/// Returns the new x0 value or a teardown signal.
pub fn dispatch(regs: &mut [u64; 8]) -> PsciAction {
    let fn_id = regs[0];
    match fn_id {
        fid::VERSION => {
            // PSCI 1.0 = 0x00010000.
            regs[0] = 0x0001_0000;
            PsciAction::Return(0x0001_0000)
        }
        fid::FEATURES => {
            // Indicate support for VERSION, CPU_OFF, SYSTEM_OFF, SYSTEM_RESET.
            let target = regs[1];
            let supported = matches!(
                target,
                fid::VERSION | fid::CPU_OFF | fid::SYSTEM_OFF | fid::SYSTEM_RESET | fid::FEATURES
            );
            let result = if supported {
                ret::SUCCESS
            } else {
                ret::NOT_SUPPORTED
            };
            regs[0] = result;
            PsciAction::Return(result)
        }
        fid::CPU_SUSPEND | fid::CPU_SUSPEND_64 => {
            // Single-CPU MVP: treat suspend as a WFI-equivalent — return SUCCESS.
            regs[0] = ret::SUCCESS;
            PsciAction::Return(ret::SUCCESS)
        }
        fid::CPU_OFF => {
            // Only one vCPU; CPU_OFF from the boot CPU = VM done.
            PsciAction::SystemOff
        }
        fid::CPU_ON | fid::CPU_ON_64 => {
            // No SMP in P05; deny additional CPU bring-up.
            regs[0] = ret::DENIED;
            PsciAction::Return(ret::DENIED)
        }
        fid::AFFINITY_INFO | fid::AFFINITY_INFO_64 => {
            // CPU 0 is always ON; all others ABSENT (we have only one vCPU).
            let mpidr = regs[1];
            let result = if mpidr == 0 { 0u64 } else { 1u64 }; // 0=ON, 1=OFF
            regs[0] = result;
            PsciAction::Return(result)
        }
        fid::SYSTEM_OFF => PsciAction::SystemOff,
        fid::SYSTEM_RESET => PsciAction::SystemReset,
        _ => {
            // Unknown PSCI / SMCCC call — return NOT_SUPPORTED.
            regs[0] = ret::NOT_SUPPORTED;
            PsciAction::Return(ret::NOT_SUPPORTED)
        }
    }
}
