// SPDX-License-Identifier: MIT
//! Intel VMX vCPU state management and execution loop.
//!
//! Owns the guest general-purpose registers (GPRs) and the VMCS frame.
//! Drives `vmx_vmrun` to execute the guest and decodes VM-exits into HAL [`ViVmExit`].
//! The VMCS is current only inside `vmcs_scope::with_vmcs` (load → use →
//! VMCLEAR), and the host-state area is rewritten from the executing CPU on
//! every run (`host_state`), so the owner task may run the vCPU on any hart.

mod cr_access;
mod host_state;
mod vmcs_scope;

use hal_hypervisor::ViVmExit;
use types::{ViError, ViResult};

use super::guest_cpuid;
use super::vmcs::*;
use super::vmx;
use super::world_switch::{vmx_vmrun, VmxGuestRegs};
use cr_access::CrOutcome;
use vmcs_scope::with_vmcs;

/// Primary controls the isolation model depends on; refused if the CPU's
/// allowed-1 settings cannot provide them. Unconditional I/O exiting keeps the
/// guest off host ports; CR8 load/store exiting keeps it off the physical TPR.
const PROC_REQUIRED: u32 = PROC_UNCONDITIONAL_IO
    | PROC_CR8_LOAD_EXITING
    | PROC_CR8_STORE_EXITING
    | PROC_ACTIVATE_SECONDARY_CONTROLS;

/// An Intel VMX virtual CPU.
pub struct VmxVcpu {
    /// Guest GPRs, indexed 0=RAX, 1=RCX, 2=RDX, 3=RBX, 4=RSP, 5=RBP, 6=RSI, 7=RDI, 8..15=R8..R15.
    pub gpr: [u64; 16],
    pub vmcs_pa: u64,
    #[allow(dead_code)]
    vmcs_va: *mut u8,
    /// Guest-visible TPR (CR8[3:0]); never reaches the physical LAPIC.
    vtpr: u8,
    /// Guest PKRU / KERNEL_GS_BASE carried across exits by the world switch.
    guest_regs: VmxGuestRegs,
    #[allow(dead_code)]
    guest_ram: *const u8,
    #[allow(dead_code)]
    guest_ram_len: usize,
}

unsafe impl Send for VmxVcpu {}

impl VmxVcpu {
    /// Create and initialize a new VMX vCPU over a caller-owned 4 KiB VMCS frame.
    ///
    /// # Safety
    /// `vmcs_pa` and `vmcs_va` must name an exclusively-owned, 4 KiB-aligned frame.
    pub unsafe fn new(
        vmcs_pa: u64,
        vmcs_va: *mut u8,
        guest_ram: *const u8,
        guest_ram_len: usize,
    ) -> ViResult<Self> {
        if vmcs_pa == 0 || vmcs_pa & 0xFFF != 0 || vmcs_va.is_null() {
            return Err(ViError::InvalidArgument);
        }

        // 1. Clear the frame
        core::ptr::write_bytes(vmcs_va, 0, 4096);

        // 2. Stamp revision ID in the first 4 bytes
        let rev = vmx::vmxon_revision();
        core::ptr::write_volatile(vmcs_va as *mut u32, rev);

        // 3. Initialise the launch state; the VMCS is loaded only per access.
        vmclear(vmcs_pa)?;

        Ok(Self {
            gpr: [0; 16],
            vmcs_pa,
            vmcs_va,
            vtpr: 0,
            guest_regs: VmxGuestRegs::default(),
            guest_ram,
            guest_ram_len,
        })
    }

    /// Physical address of the underlying VMCS frame.
    #[inline]
    pub fn vmcs_pa(&self) -> u64 {
        self.vmcs_pa
    }

    /// Configure the initial VMCS state for direct PVH boot (32-bit flat protected mode).
    ///
    /// # Arguments
    /// * `entry_pc` - 32-bit guest entry RIP (`PHYS32_ENTRY`).
    /// * `hvm_info_gpa` - physical address of `hvm_start_info` struct (passed in `%ebx`).
    /// * `eptp` - formatted EPTP control value from [`NestedPageTable::eptp`].
    /// * `host_cr3` - host PML4 root for the initial host-state area; every
    ///   [`Self::run`] replaces it with the live CR3 of the running CPU.
    ///
    /// Fails with [`ViError::NotSupported`] when the CPU cannot provide the
    /// exit controls the isolation model needs (external-interrupt exiting,
    /// unconditional I/O exiting, CR8 load/store exiting, EPT).
    ///
    /// # Safety
    /// The CPU must be in VMX root operation, since this loads this vCPU's VMCS
    /// and writes host state read from the live CR0/CR3/CR4 and segment
    /// registers. `eptp` must name an EPT hierarchy that stays valid while the
    /// guest runs, and `host_cr3` must map the kernel's VM-exit path.
    pub unsafe fn init_pvh(
        &mut self,
        entry_pc: u64,
        hvm_info_gpa: u64,
        eptp: u64,
        host_cr3: u64,
    ) -> ViResult<()> {
        with_vmcs(self.vmcs_pa, || {
            host_state::refresh(host_cr3)?;
            program_pvh(entry_pc, eptp)
        })?;
        // PVH ABI: %ebx points to hvm_start_info
        self.gpr[3] = hvm_info_gpa; // index 3 = RBX
        Ok(())
    }
}

/// Write the create-once VMCS fields: static host fields, controls, PVH guest
/// state.
///
/// # Safety
/// VMX root operation with the target VMCS current.
unsafe fn program_pvh(entry_pc: u64, eptp: u64) -> ViResult<()> {
    vmwrite(HOST_IA32_SYSENTER_CS, 0)?;
    vmwrite(HOST_SYSENTER_ESP, 0)?;
    vmwrite(HOST_SYSENTER_EIP, 0)?;
    // ── 2. Control Fields Setup ──────────────────────────────────────────
    let use_true = has_true_ctls();
    let pin_msr = if use_true {
        MSR_IA32_VMX_TRUE_PINBASED_CTLS
    } else {
        MSR_IA32_VMX_PINBASED_CTLS
    };
    let proc_msr = if use_true {
        MSR_IA32_VMX_TRUE_PROCBASED_CTLS
    } else {
        MSR_IA32_VMX_PROCBASED_CTLS
    };
    let exit_msr = if use_true {
        MSR_IA32_VMX_TRUE_EXIT_CTLS
    } else {
        MSR_IA32_VMX_EXIT_CTLS
    };
    let entry_msr = if use_true {
        MSR_IA32_VMX_TRUE_ENTRY_CTLS
    } else {
        MSR_IA32_VMX_ENTRY_CTLS
    };

    let pin_ctls = adjust_vmx_ctrl(PIN_EXTERNAL_INTERRUPT, pin_msr);
    if pin_ctls & PIN_EXTERNAL_INTERRUPT == 0 {
        return Err(ViError::NotSupported);
    }
    vmwrite(PIN_BASED_VM_EXEC_CONTROL, pin_ctls as u64)?;

    let proc_ctls = adjust_vmx_ctrl(PROC_HLT_EXITING | PROC_REQUIRED, proc_msr);
    if proc_ctls & PROC_REQUIRED != PROC_REQUIRED || proc_ctls & PROC_USE_TPR_SHADOW != 0 {
        return Err(ViError::NotSupported);
    }
    vmwrite(CPU_BASED_VM_EXEC_CONTROL, proc_ctls as u64)?;

    let sec_ctls = adjust_vmx_ctrl(
        SEC_ENABLE_EPT | SEC_ENABLE_RDTSCP | SEC_UNRESTRICTED_GUEST,
        MSR_IA32_VMX_PROCBASED_CTLS2,
    );
    if sec_ctls & SEC_ENABLE_EPT == 0 {
        return Err(ViError::NotSupported);
    }
    vmwrite(SECONDARY_VM_EXEC_CONTROL, sec_ctls as u64)?;

    // No EXIT_ACKNOWLEDGE_INTERRUPT: an external-interrupt exit leaves the
    // vector pending in the LAPIC IRR, so the host IDT takes it (and EOIs it)
    // once IF is re-enabled. Acking on exit would park it in the ISR with no
    // handler and no EOI, masking every lower-priority vector.
    let exit_ctls = adjust_vmx_ctrl(EXIT_HOST_ADDR_SPACE_SIZE, exit_msr);
    vmwrite(VM_EXIT_CONTROLS, exit_ctls as u64)?;

    // VM entry: 32-bit guest (IA-32e mode guest bit = 0)
    let entry_ctls = adjust_vmx_ctrl(0, entry_msr);
    vmwrite(VM_ENTRY_CONTROLS, entry_ctls as u64)?;

    vmwrite(EPT_POINTER, eptp)?;
    vmwrite(VMCS_LINK_POINTER, !0u64)?;

    // Clear all event injection and MSR load/store counts
    vmwrite(EXCEPTION_BITMAP, 0)?;
    vmwrite(PAGE_FAULT_ERROR_CODE_MASK, 0)?;
    vmwrite(PAGE_FAULT_ERROR_CODE_MATCH, 0)?;
    vmwrite(CR3_TARGET_COUNT, 0)?;
    vmwrite(VM_EXIT_MSR_STORE_COUNT, 0)?;
    vmwrite(VM_EXIT_MSR_LOAD_COUNT, 0)?;
    vmwrite(VM_ENTRY_MSR_LOAD_COUNT, 0)?;
    vmwrite(VM_ENTRY_INTR_INFO_FIELD, 0)?;
    vmwrite(VM_ENTRY_EXCEPTION_ERROR_CODE, 0)?;
    vmwrite(VM_ENTRY_INSTRUCTION_LEN, 0)?;
    vmwrite(TPR_THRESHOLD, 0)?;
    vmwrite(TSC_OFFSET, 0)?;
    vmwrite(GUEST_DR7, 0x400)?; // Bit 10 is reserved (must be 1)
    vmwrite(GUEST_PENDING_DBG_EXCEPTIONS, 0)?;
    vmwrite(GUEST_SYSENTER_CS, 0)?;
    vmwrite(GUEST_SYSENTER_ESP, 0)?;
    vmwrite(GUEST_SYSENTER_EIP, 0)?;
    vmwrite(GUEST_IA32_DEBUGCTL, 0)?;
    vmwrite(GUEST_IA32_PAT, 0x0007_0406_0007_0406)?;
    vmwrite(GUEST_IA32_EFER, 0)?;
    vmwrite(GUEST_IA32_PERF_GLOBAL_CTRL, 0)?;

    let mut host_efer: u64 = 0;
    core::arch::asm!("rdmsr", in("ecx") 0xC000_0080u32, out("eax") host_efer, out("edx") _, options(nomem, nostack));
    vmwrite(HOST_IA32_PAT, 0x0007_0406_0007_0406)?;
    vmwrite(HOST_IA32_EFER, host_efer)?;
    vmwrite(HOST_IA32_PERF_GLOBAL_CTRL, 0)?;
    // ── 3. Guest State Setup (PVH 32-bit Protected Mode) ─────────────────
    // Linux PVH entry: CR0.PE=1, CR0.ET=1, CR0.NE=1 (0x31), CR4=0, EFER=0, paging off
    vmwrite(GUEST_CR0, 0x31)?;
    vmwrite(CR0_READ_SHADOW, 0x31)?;
    vmwrite(GUEST_CR3, 0)?;
    let mut cr4_f0: u32 = 0;
    core::arch::asm!("rdmsr", in("ecx") 0x488u32, out("eax") cr4_f0, out("edx") _, options(nomem, nostack));
    vmwrite(GUEST_CR4, cr4_f0 as u64)?;
    vmwrite(GUEST_RFLAGS, 0x2)?; // Bit 1 is always 1 in RFLAGS
    vmwrite(GUEST_RIP, entry_pc)?;
    vmwrite(GUEST_RSP, 0)?;
    vmwrite(GUEST_DR7, 0x400)?; // Bit 10 is reserved (must be 1)
    vmwrite(GUEST_PENDING_DBG_EXCEPTIONS, 0)?;
    vmwrite(GUEST_SYSENTER_CS, 0)?;
    vmwrite(GUEST_SYSENTER_ESP, 0)?;
    vmwrite(GUEST_SYSENTER_EIP, 0)?;
    // CS: 32-bit flat code segment
    vmwrite(GUEST_CS_SELECTOR, 0x10)?;
    vmwrite(GUEST_CS_BASE, 0)?;
    vmwrite(GUEST_CS_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(GUEST_CS_AR_BYTES, 0xC09B)?; // Present, Code, Readable, 32-bit (DB=1), 4KB gran (G=1)

    // DS, ES, SS, FS, GS: 32-bit flat data segments
    for &sel in &[
        (
            GUEST_DS_SELECTOR,
            GUEST_DS_BASE,
            GUEST_DS_LIMIT,
            GUEST_DS_AR_BYTES,
        ),
        (
            GUEST_ES_SELECTOR,
            GUEST_ES_BASE,
            GUEST_ES_LIMIT,
            GUEST_ES_AR_BYTES,
        ),
        (
            GUEST_SS_SELECTOR,
            GUEST_SS_BASE,
            GUEST_SS_LIMIT,
            GUEST_SS_AR_BYTES,
        ),
        (
            GUEST_FS_SELECTOR,
            GUEST_FS_BASE,
            GUEST_FS_LIMIT,
            GUEST_FS_AR_BYTES,
        ),
        (
            GUEST_GS_SELECTOR,
            GUEST_GS_BASE,
            GUEST_GS_LIMIT,
            GUEST_GS_AR_BYTES,
        ),
    ] {
        vmwrite(sel.0, 0x18)?;
        vmwrite(sel.1, 0)?;
        vmwrite(sel.2, 0xFFFF_FFFF)?;
        vmwrite(sel.3, 0xC093)?; // Present, Data, Writable, 32-bit (DB=1), 4KB gran (G=1)
    }

    // TR: busy 32-bit TSS (must not be null in VMX)
    vmwrite(GUEST_TR_SELECTOR, 0x20)?;
    vmwrite(GUEST_TR_BASE, 0)?;
    vmwrite(GUEST_TR_LIMIT, 0x67)?; // >= 0x67 required by Intel SDM for 32-bit TSS
    vmwrite(GUEST_TR_AR_BYTES, 0x008B)?; // Present, 32-bit TSS

    // LDTR: unusable
    vmwrite(GUEST_LDTR_SELECTOR, 0)?;
    vmwrite(GUEST_LDTR_BASE, 0)?;
    vmwrite(GUEST_LDTR_LIMIT, 0)?;
    vmwrite(GUEST_LDTR_AR_BYTES, 0x10000)?; // Bit 16 = unusable

    // GDTR & IDTR
    vmwrite(GUEST_GDTR_BASE, 0)?;
    vmwrite(GUEST_GDTR_LIMIT, 0)?;
    vmwrite(GUEST_IDTR_BASE, 0)?;
    vmwrite(GUEST_IDTR_LIMIT, 0)?;

    vmwrite(GUEST_INTERRUPTIBILITY_INFO, 0)?;
    vmwrite(GUEST_ACTIVITY_STATE, 0)?;
    Ok(())
}

impl VmxVcpu {
    /// Run the guest vCPU until an exit requires hypervisor handling.
    ///
    /// Loads the VMCS, refreshes the host-state area from this CPU, runs, and
    /// VMCLEARs before returning, all with IF=0, so successive calls may come
    /// from different CPUs.
    pub fn run(&mut self) -> ViResult<ViVmExit> {
        // SAFETY: VMX root is active whenever a VmxVcpu exists; the scope owns
        // VMCS currency and keeps IF=0 across refresh + world switch.
        unsafe {
            with_vmcs(self.vmcs_pa, || {
                host_state::refresh(host_state::live_cr3())?;
                self.run_loaded()
            })
        }
    }

    /// # Safety
    /// This vCPU's VMCS is current with a fresh host-state area, IF=0.
    unsafe fn run_loaded(&mut self) -> ViResult<ViVmExit> {
        // The scope VMCLEARed the VMCS since the last run: launch state clear.
        let mut launched = false;
        loop {
            let status = vmx_vmrun(self.gpr.as_mut_ptr(), launched as u64, &mut self.guest_regs);
            if status != 0 {
                return Ok(ViVmExit::Shutdown);
            }
            launched = true;

            let exit_reason = (vmread(VM_EXIT_REASON)? & 0xFFFF) as u32;
            let insn_len = vmread(VM_EXIT_INSTRUCTION_LEN).unwrap_or(0);
            let guest_rip = vmread(GUEST_RIP)?;
            let advance = || vmwrite(GUEST_RIP, guest_rip + insn_len);

            match exit_reason {
                EXIT_REASON_HLT => {
                    let _ = advance();
                    return Ok(ViVmExit::Hlt);
                }
                EXIT_REASON_EXTERNAL_INTERRUPT => {
                    return Ok(ViVmExit::Preempted);
                }
                EXIT_REASON_IO_INSTRUCTION => {
                    let qual = vmread(EXIT_QUALIFICATION)?;
                    let size = (qual & 0x7) as u8 + 1; // 0=1 byte, 1=2 bytes, 3=4 bytes
                    let is_in = (qual & (1 << 3)) != 0;
                    let port = ((qual >> 16) & 0xFFFF) as u16;
                    let _ = advance();

                    if is_in {
                        return Ok(ViVmExit::PortIn { port, size });
                    }
                    let mask = match size {
                        1 => 0xFF,
                        2 => 0xFFFF,
                        _ => 0xFFFF_FFFF,
                    };
                    let val = (self.gpr[0] & mask) as u32;
                    return Ok(ViVmExit::PortOut { port, size, val });
                }
                EXIT_REASON_EPT_VIOLATION => {
                    let gpa = vmread(GUEST_PHYSICAL_ADDRESS)?;
                    let qual = vmread(EXIT_QUALIFICATION)?;
                    let is_write = (qual & (1 << 1)) != 0;
                    let _ = advance();

                    if is_write {
                        return Ok(ViVmExit::MmioWrite {
                            ipa: gpa,
                            size: 4,
                            val: self.gpr[0],
                        });
                    }
                    return Ok(ViVmExit::MmioRead {
                        ipa: gpa,
                        size: 4,
                        reg: 0,
                    });
                }
                EXIT_REASON_CPUID => {
                    let [a, b, c, d] = guest_cpuid::query(self.gpr[0] as u32, self.gpr[1] as u32);
                    self.gpr[0] = u64::from(a); // RAX
                    self.gpr[3] = u64::from(b); // RBX
                    self.gpr[1] = u64::from(c); // RCX
                    self.gpr[2] = u64::from(d); // RDX
                    let _ = advance();
                    continue;
                }
                EXIT_REASON_CR_ACCESS => {
                    let qual = vmread(EXIT_QUALIFICATION)?;
                    if let CrOutcome::Done = self.handle_cr_access(qual)? {
                        let _ = advance();
                    }
                    continue;
                }
                EXIT_REASON_TRIPLE_FAULT => {
                    return Ok(ViVmExit::Shutdown);
                }
                other => {
                    let _ = advance();
                    return Ok(ViVmExit::Unknown {
                        ec: other,
                        iss: insn_len as u32,
                    });
                }
            }
        }
    }
}
