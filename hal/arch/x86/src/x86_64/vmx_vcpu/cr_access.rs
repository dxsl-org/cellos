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
use super::{queue_gp, VmxVcpu, CR0_CD, CR0_NW, CR0_PE, CR0_PG};

const ACCESS_MOV_TO_CR: u64 = 0;
const ACCESS_MOV_FROM_CR: u64 = 1;
const REG_RSP: usize = 4;
const CR4_PAE: u64 = 1 << 5;
const CR4_PCIDE: u64 = 1 << 17;
const EFER_LME: u64 = 1 << 8;
const EFER_LMA: u64 = 1 << 10;
/// Guest CS access rights: L (64-bit code segment).
const CS_AR_L: u64 = 1 << 13;

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
                    queue_gp()?;
                    return Ok(CrOutcome::Faulted);
                }
                self.vtpr = value as u8;
            }
            (8, ACCESS_MOV_FROM_CR) => self.write_gpr(reg, u64::from(self.vtpr))?,
            (0, ACCESS_MOV_TO_CR) => {
                // Only writes that change a host-owned bit (NE, CD, NW)
                // against the read shadow exit here. The write is applied by
                // VMWRITE, so the long-mode switch the CPU would make on a
                // native PG=1 write (EFER.LMA from LME) is made here, after the
                // #GP checks the CPU would make.
                let value = self.read_gpr(reg)?;
                let allowed = super::super::msr::read(MSR_IA32_VMX_CR0_FIXED1);
                let efer = vmread(GUEST_IA32_EFER)?;
                let paging = value & CR0_PG != 0;
                let enters_long_mode = paging && efer & EFER_LME != 0;
                let leaves_paging_in_64bit_code = !paging
                    && efer & EFER_LMA != 0
                    && vmread(GUEST_CS_AR_BYTES)? & CS_AR_L != 0;
                if value & !allowed != 0
                    || (paging && value & CR0_PE == 0)
                    || (value & CR0_NW != 0 && value & CR0_CD == 0)
                    || (enters_long_mode && vmread(GUEST_CR4)? & CR4_PAE == 0)
                    || leaves_paging_in_64bit_code
                {
                    queue_gp()?;
                    return Ok(CrOutcome::Faulted);
                }
                let lma = if enters_long_mode { EFER_LMA } else { 0 };
                vmwrite(GUEST_IA32_EFER, (efer & !EFER_LMA) | lma)?;
                // CD/NW stay 0 in hardware; the guest keeps its values in the shadow.
                let fixed1 = vmread(CR0_GUEST_HOST_MASK)? & !(CR0_CD | CR0_NW);
                vmwrite(GUEST_CR0, (value | fixed1) & !(CR0_CD | CR0_NW))?;
                vmwrite(CR0_READ_SHADOW, value)?;
            }
            (4, ACCESS_MOV_TO_CR) => {
                // Only writes that change a host-owned bit (the VMX fixed-1
                // bits, e.g. VMXE) against the read shadow exit here. The
                // guest sees its own value; the CPU keeps the fixed-1 bits.
                let value = self.read_gpr(reg)?;
                let allowed = super::super::msr::read(MSR_IA32_VMX_CR4_FIXED1);
                let long_mode = vmread(GUEST_IA32_EFER)? & EFER_LMA != 0;
                let enables_pcide =
                    value & CR4_PCIDE != 0 && vmread(GUEST_CR4)? & CR4_PCIDE == 0;
                let pcide_refused =
                    enables_pcide && (!long_mode || vmread(GUEST_CR3)? & 0xFFF != 0);
                if value & !allowed != 0 || (long_mode && value & CR4_PAE == 0) || pcide_refused {
                    queue_gp()?;
                    return Ok(CrOutcome::Faulted);
                }
                let fixed1 = vmread(CR4_GUEST_HOST_MASK)?;
                vmwrite(GUEST_CR4, value | fixed1)?;
                vmwrite(CR4_READ_SHADOW, value)?;
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
