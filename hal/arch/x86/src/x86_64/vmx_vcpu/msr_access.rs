// SPDX-License-Identifier: MIT
//! `RDMSR`/`WRMSR` exits (`EXIT_REASON_MSR_READ`/`_WRITE`).
//!
//! No MSR bitmap is installed, so every guest MSR access exits and is emulated
//! here; the guest never reaches a physical MSR. Guest-context MSRs live where
//! the hardware takes them from on entry: EFER, FS/GS base and SYSENTER in the
//! VMCS guest area, KERNEL_GS_BASE and the SYSCALL MSRs in [`VmxGuestRegs`],
//! which `vmx_vmrun` loads around the world switch; `IA32_TSC_DEADLINE` is the
//! local APIC's timer (`lapic`). Every other read returns 0 and every other
//! write is dropped, as on the SVM backend.

use types::ViResult;

use super::super::vmcs::*;
use super::super::world_switch::SyscallMsr;
use super::lapic::IA32_TSC_DEADLINE;
use super::{queue_gp, VmxVcpu, CR0_PG};

const MSR_APIC_BASE: u32 = 0x1B;
const MSR_SYSENTER_CS: u32 = 0x174;
const MSR_SYSENTER_ESP: u32 = 0x175;
const MSR_SYSENTER_EIP: u32 = 0x176;
const MSR_EFER: u32 = 0xC000_0080;
const MSR_STAR: u32 = 0xC000_0081;
const MSR_LSTAR: u32 = 0xC000_0082;
const MSR_CSTAR: u32 = 0xC000_0083;
const MSR_SFMASK: u32 = 0xC000_0084;
const MSR_FS_BASE: u32 = 0xC000_0100;
const MSR_GS_BASE: u32 = 0xC000_0101;
const MSR_KERNEL_GS_BASE: u32 = 0xC000_0102;

/// xAPIC enabled at 0xFEE00000, BSP; no x2APIC (the guest reaches its local
/// APIC through the register page).
const APIC_BASE_VALUE: u64 = 0xFEE0_0000 | (1 << 8) | (1 << 11);
/// EFER bits the guest may set: SCE, LME, NXE. LMA is hardware-owned.
const EFER_GUEST_WRITABLE: u64 = (1 << 0) | (1 << 8) | (1 << 11);
const EFER_LME: u64 = 1 << 8;
const EFER_LMA: u64 = 1 << 10;

/// Result of emulating one MSR exit.
pub(super) enum MsrOutcome {
    /// Emulated; advance RIP past the instruction.
    Done,
    /// #GP queued for the next entry; RIP stays on the instruction.
    Faulted,
}

fn is_canonical(value: u64) -> bool {
    ((value as i64) << 16 >> 16) as u64 == value
}

impl VmxVcpu {
    /// Emulate the `RDMSR` (`write == false`) or `WRMSR` at the current exit.
    ///
    /// # Safety
    /// This vCPU's VMCS is current (inside the run scope).
    pub(super) unsafe fn handle_msr(&mut self, write: bool) -> ViResult<MsrOutcome> {
        let index = self.gpr[1] as u32;
        if !write {
            let value = self.read_msr(index)?;
            self.gpr[0] = value & 0xFFFF_FFFF;
            self.gpr[2] = value >> 32;
            return Ok(MsrOutcome::Done);
        }
        let value = ((self.gpr[2] & 0xFFFF_FFFF) << 32) | (self.gpr[0] & 0xFFFF_FFFF);
        if self.write_msr(index, value)? {
            Ok(MsrOutcome::Done)
        } else {
            queue_gp()?;
            Ok(MsrOutcome::Faulted)
        }
    }

    unsafe fn read_msr(&self, index: u32) -> ViResult<u64> {
        Ok(match index {
            MSR_EFER => vmread(GUEST_IA32_EFER)?,
            MSR_APIC_BASE => APIC_BASE_VALUE,
            MSR_FS_BASE => vmread(GUEST_FS_BASE)?,
            MSR_GS_BASE => vmread(GUEST_GS_BASE)?,
            MSR_KERNEL_GS_BASE => self.guest_regs.loaded_kernel_gs_base(),
            IA32_TSC_DEADLINE => self.lapic.deadline(),
            MSR_SYSENTER_CS => vmread(GUEST_SYSENTER_CS)?,
            MSR_SYSENTER_ESP => vmread(GUEST_SYSENTER_ESP)?,
            MSR_SYSENTER_EIP => vmread(GUEST_SYSENTER_EIP)?,
            _ => match syscall_slot(index) {
                Some(slot) => self.guest_regs.syscall_msr(slot),
                None => 0,
            },
        })
    }

    /// Returns `false` when the write must raise #GP.
    unsafe fn write_msr(&mut self, index: u32, value: u64) -> ViResult<bool> {
        if !value_is_loadable(index, value) {
            return Ok(false);
        }
        match index {
            MSR_EFER => {
                let efer = vmread(GUEST_IA32_EFER)?;
                // Changing LME with paging on is #GP on hardware; accepting it
                // would only fail the next VM entry.
                if (value ^ efer) & EFER_LME != 0 && vmread(GUEST_CR0)? & CR0_PG != 0 {
                    return Ok(false);
                }
                vmwrite(
                    GUEST_IA32_EFER,
                    (value & EFER_GUEST_WRITABLE) | (efer & EFER_LMA),
                )?;
            }
            MSR_FS_BASE => vmwrite(GUEST_FS_BASE, value)?,
            MSR_GS_BASE => vmwrite(GUEST_GS_BASE, value)?,
            MSR_KERNEL_GS_BASE => self.guest_regs.set_loaded_kernel_gs_base(value),
            IA32_TSC_DEADLINE => self.lapic.set_deadline(value),
            MSR_SYSENTER_CS => vmwrite(GUEST_SYSENTER_CS, value & 0xFFFF_FFFF)?,
            MSR_SYSENTER_ESP => vmwrite(GUEST_SYSENTER_ESP, value)?,
            MSR_SYSENTER_EIP => vmwrite(GUEST_SYSENTER_EIP, value)?,
            _ => {
                if let Some(slot) = syscall_slot(index) {
                    self.guest_regs.set_loaded_syscall_msr(slot, value);
                }
            }
        }
        Ok(true)
    }
}

/// Whether `value` may be stored for MSR `index`. Stored addresses end up in a
/// VMCS guest field (a non-canonical one fails VM entry) or are loaded into
/// the physical MSR around the world switch, where a non-canonical address or
/// a reserved SFMASK bit (63:32) would #GP in the host.
fn value_is_loadable(index: u32, value: u64) -> bool {
    match index {
        MSR_FS_BASE | MSR_GS_BASE | MSR_KERNEL_GS_BASE | MSR_LSTAR | MSR_CSTAR
        | MSR_SYSENTER_ESP | MSR_SYSENTER_EIP => is_canonical(value),
        MSR_SFMASK => value >> 32 == 0,
        _ => true,
    }
}

fn syscall_slot(index: u32) -> Option<SyscallMsr> {
    Some(match index {
        MSR_STAR => SyscallMsr::Star,
        MSR_LSTAR => SyscallMsr::Lstar,
        MSR_CSTAR => SyscallMsr::Cstar,
        MSR_SFMASK => SyscallMsr::Sfmask,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_that_would_fault_the_host_or_entry_are_refused() {
        // Reserved SFMASK bits would #GP the host's WRMSR before entry.
        assert!(!value_is_loadable(MSR_SFMASK, 1 << 32));
        assert!(value_is_loadable(MSR_SFMASK, 0x4_7700));
        // Addresses must be canonical (bit 47 sign-extended).
        assert!(value_is_loadable(MSR_LSTAR, 0xffff_8000_0000_0000));
        assert!(value_is_loadable(MSR_LSTAR, 0x0000_7fff_ffff_ffff));
        assert!(!value_is_loadable(MSR_LSTAR, 0x0000_8000_0000_0000));
        assert!(!value_is_loadable(
            MSR_KERNEL_GS_BASE,
            0xfff0_0000_0000_0000
        ));
        assert!(!value_is_loadable(MSR_SYSENTER_EIP, 0x0000_8000_0000_0000));
        // STAR holds selectors, not an address; any value loads.
        assert!(value_is_loadable(MSR_STAR, u64::MAX));
    }
}
