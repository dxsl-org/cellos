// SPDX-License-Identifier: MIT
//! Control-register access exits (`EXIT_REASON_CR_ACCESS`).
//!
//! CR8 load/store exiting is mandatory in this VMCS: with neither it nor a TPR
//! shadow, a 64-bit guest's `mov cr8` writes the physical LAPIC TPR, which no
//! VM exit restores — `mov cr8, 15` would mask the host timer and IPIs on that
//! CPU for good. Instead the guest owns a virtual TPR (`VmxVcpu::vtpr`) that
//! only gates the VMM's own interrupt injection.

use types::ViResult;

use super::super::vmcs::*;
use super::VmxVcpu;

const ACCESS_MOV_TO_CR: u64 = 0;
const ACCESS_MOV_FROM_CR: u64 = 1;
const REG_RSP: usize = 4;
/// VM-entry interruption info: valid, hardware exception, error code, #GP.
const INJECT_GP: u64 = (1 << 31) | (1 << 11) | (3 << 8) | 13;

/// Result of emulating one CR-access exit.
pub(super) enum CrOutcome {
    /// Instruction emulated; advance RIP past it.
    Done,
    /// A fault was queued for the next entry; RIP stays on the instruction.
    Faulted,
}

impl VmxVcpu {
    /// Emulate the CR access described by exit qualification `qual`.
    ///
    /// # Safety
    /// This vCPU's VMCS is current (inside the run scope).
    pub(super) unsafe fn handle_cr_access(&mut self, qual: u64) -> ViResult<CrOutcome> {
        let cr = qual & 0xF;
        let access = (qual >> 4) & 0x3;
        let reg = ((qual >> 8) & 0xF) as usize;
        match (cr, access) {
            (8, ACCESS_MOV_TO_CR) => {
                let value = self.read_gpr(reg)?;
                if value > 0xF {
                    // SDM: MOV to CR8 with bits 63:4 set raises #GP(0).
                    vmwrite(VM_ENTRY_INTR_INFO_FIELD, INJECT_GP)?;
                    vmwrite(VM_ENTRY_EXCEPTION_ERROR_CODE, 0)?;
                    return Ok(CrOutcome::Faulted);
                }
                self.vtpr = value as u8;
            }
            (8, ACCESS_MOV_FROM_CR) => self.write_gpr(reg, u64::from(self.vtpr))?,
            (0, ACCESS_MOV_TO_CR) => {
                // CR0 write turning on paging: enable IA-32e mode on entry.
                let value = self.read_gpr(reg)?;
                if value & (1 << 31) != 0 {
                    let entry_msr = if has_true_ctls() {
                        MSR_IA32_VMX_TRUE_ENTRY_CTLS
                    } else {
                        MSR_IA32_VMX_ENTRY_CTLS
                    };
                    let entry_ctls = adjust_vmx_ctrl(ENTRY_IA32E_MODE_GUEST, entry_msr);
                    vmwrite(VM_ENTRY_CONTROLS, u64::from(entry_ctls))?;
                    vmwrite(GUEST_CR0, value)?;
                    vmwrite(CR0_READ_SHADOW, value)?;
                }
            }
            // Other CR exits are not configured; skipping the instruction
            // keeps the pre-existing behavior for capability-forced exits.
            _ => {}
        }
        Ok(CrOutcome::Done)
    }

    unsafe fn read_gpr(&self, reg: usize) -> ViResult<u64> {
        if reg == REG_RSP {
            vmread(GUEST_RSP)
        } else {
            Ok(self.gpr[reg])
        }
    }

    unsafe fn write_gpr(&mut self, reg: usize, value: u64) -> ViResult<()> {
        if reg == REG_RSP {
            vmwrite(GUEST_RSP, value)
        } else {
            self.gpr[reg] = value;
            Ok(())
        }
    }
}
