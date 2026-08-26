//! Bounded Silo VM-exit dispatch.

use crate::vmm;
use api::hypervisor::ViVmExit;
use service_silo::layout::{HVC_SILO_DONE, HVC_SILO_FAULT, HVC_SILO_READY};
use service_silo::vm_exit::{diagnose_hvc, UnexpectedHvc};


const MAX_EXITS_PER_OPERATION: usize = 64;

/// Non-secret diagnostic fields for an unexpected, always-fatal VM exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiloUnexpectedExit {
    MmioRead { ipa: u64, size: u8, reg: u8 },
    MmioWrite { ipa: u64, size: u8 },
    Hvc(UnexpectedHvc),
    SysReg {
        op0: u8,
        op1: u8,
        crn: u8,
        crm: u8,
        op2: u8,
        rt: u8,
        is_write: bool,
    },
    Shutdown,
    Unknown { ec: u32, iss: u32, pc: Option<u64> },
    PortIn { port: u16, size: u8, reg: u8 },
    PortOut { port: u16, size: u8 },
    Hlt,
    Msr { index: u32, is_write: bool },
}

/// A bounded VMM-side failure, distinct from a guest-declared Silo fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiloVmmFault {
    RunVcpu,
    UnexpectedExit(SiloUnexpectedExit),
    ExitBudgetExceeded,
}

/// Result of one bounded guest execution interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiloRunResult {
    Done,
    /// Guest-declared protocol/crypto fault code carried in HVC x1.
    GuestFault(u64),
    VmmFault(SiloVmmFault),
}

/// Run until the purpose-specific completion HVC or a bounded failure occurs.
pub fn run_until_done(vm_id: usize, vcpu_id: usize) -> SiloRunResult {
    let mut exit = ViVmExit::Unknown { ec: 0, iss: 0 };
    for _ in 0..MAX_EXITS_PER_OPERATION {
        if vmm::run_vcpu(vm_id, vcpu_id, &mut exit) == usize::MAX {
            return SiloRunResult::VmmFault(SiloVmmFault::RunVcpu);
        }
        match exit {
            ViVmExit::Hvc { imm: 0, regs }
                if regs[0] == HVC_SILO_READY || regs[0] == HVC_SILO_DONE =>
            {
                return SiloRunResult::Done;
            }
            ViVmExit::Hvc { imm: 0, regs } if regs[0] == HVC_SILO_FAULT => {
                return SiloRunResult::GuestFault(regs[1]);
            }
            ViVmExit::Wfi | ViVmExit::Preempted => {}
            unexpected => {
                return SiloRunResult::VmmFault(SiloVmmFault::UnexpectedExit(
                    diagnose_unexpected_exit(vm_id, vcpu_id, unexpected),
                ));
            }
        }
    }
    SiloRunResult::VmmFault(SiloVmmFault::ExitBudgetExceeded)
}

fn diagnose_unexpected_exit(
    vm_id: usize,
    vcpu_id: usize,
    exit: ViVmExit,
) -> SiloUnexpectedExit {
    match exit {
        ViVmExit::MmioRead { ipa, size, reg } => {
            SiloUnexpectedExit::MmioRead { ipa, size, reg }
        }
        ViVmExit::MmioWrite { ipa, size, .. } => SiloUnexpectedExit::MmioWrite { ipa, size },
        ViVmExit::Hvc { imm, regs } => SiloUnexpectedExit::Hvc(diagnose_hvc(imm, regs)),
        ViVmExit::SysReg {
            op0,
            op1,
            crn,
            crm,
            op2,
            rt,
            is_write,
        } => SiloUnexpectedExit::SysReg {
            op0,
            op1,
            crn,
            crm,
            op2,
            rt,
            is_write,
        },
        ViVmExit::Shutdown => SiloUnexpectedExit::Shutdown,
        ViVmExit::Unknown { ec, iss } => {
            let mut regs = [0u64; 32];
            let pc = if vmm::read_vcpu_regs(vm_id, vcpu_id, &mut regs) == 0 {
                Some(regs[31])
            } else {
                None
            };
            regs.fill(0);
            core::hint::black_box(&regs);
            SiloUnexpectedExit::Unknown { ec, iss, pc }
        }
        ViVmExit::PortIn { port, size, reg } => {
            SiloUnexpectedExit::PortIn { port, size, reg }
        }
        ViVmExit::PortOut { port, size, .. } => SiloUnexpectedExit::PortOut { port, size },
        ViVmExit::Hlt => SiloUnexpectedExit::Hlt,
        ViVmExit::Msr {
            index, is_write, ..
        } => SiloUnexpectedExit::Msr { index, is_write },
        ViVmExit::Wfi | ViVmExit::Preempted => unreachable!(),
    }
}
