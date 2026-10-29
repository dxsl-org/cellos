// SPDX-License-Identifier: MIT
//! Intel VMX vCPU state management and execution loop.
//!
//! Owns the guest general-purpose registers (GPRs) and the active VMCS frame.
//! Drives `vmx_vmrun` to execute the guest and decodes VM-exits into HAL [`ViVmExit`].

use hal_hypervisor::ViVmExit;
use types::{ViError, ViResult};

use super::vmcs::*;
use super::vmx;
use super::world_switch::vmx_vmrun;

/// An Intel VMX virtual CPU.
pub struct VmxVcpu {
    /// Guest GPRs, indexed 0=RAX, 1=RCX, 2=RDX, 3=RBX, 4=RSP, 5=RBP, 6=RSI, 7=RDI, 8..15=R8..R15.
    pub gpr: [u64; 16],
    pub vmcs_pa: u64,
    #[allow(dead_code)]
    vmcs_va: *mut u8,
    launched: bool,
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

        // 3. Clear and load VMCS
        vmclear(vmcs_pa)?;
        vmptrld(vmcs_pa)?;

        Ok(Self {
            gpr: [0; 16],
            vmcs_pa,
            vmcs_va,
            launched: false,
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
    /// * `host_cr3` - host PML4 root physical address.
    pub unsafe fn init_pvh(
        &mut self,
        entry_pc: u64,
        hvm_info_gpa: u64,
        eptp: u64,
        host_cr3: u64,
    ) -> ViResult<()> {
        vmptrld(self.vmcs_pa)?;

        // ── 1. Host State Setup ──────────────────────────────────────────────
        let mut host_cr0: u64;
        let mut host_cr4: u64;
        core::arch::asm!("mov {0}, cr0", out(reg) host_cr0, options(nomem, nostack));
        core::arch::asm!("mov {0}, cr4", out(reg) host_cr4, options(nomem, nostack));

        vmwrite(HOST_CR0, host_cr0)?;
        vmwrite(HOST_CR3, host_cr3)?;
        vmwrite(HOST_CR4, host_cr4)?;

        // Read host segment selectors
        let mut host_cs: u16;
        let mut host_ds: u16;
        let mut host_es: u16;
        let mut host_ss: u16;
        let mut host_tr: u16;
        core::arch::asm!(
            "mov {cs:x}, cs",
            "mov {ds:x}, ds",
            "mov {es:x}, es",
            "mov {ss:x}, ss",
            "str {tr:x}",
            cs = out(reg) host_cs,
            ds = out(reg) host_ds,
            es = out(reg) host_es,
            ss = out(reg) host_ss,
            tr = out(reg) host_tr,
            options(nomem, nostack),
        );

        vmwrite(HOST_CS_SELECTOR, host_cs as u64)?;
        vmwrite(HOST_DS_SELECTOR, host_ds as u64)?;
        vmwrite(HOST_ES_SELECTOR, host_es as u64)?;
        vmwrite(HOST_SS_SELECTOR, host_ss as u64)?;
        vmwrite(HOST_FS_SELECTOR, 0)?;
        vmwrite(HOST_GS_SELECTOR, 0)?;
        vmwrite(HOST_TR_SELECTOR, host_tr as u64)?;

        // Read host descriptor tables
        #[repr(C, packed)]
        struct Dtr {
            limit: u16,
            base: u64,
        }
        let mut gdtr = Dtr { limit: 0, base: 0 };
        let mut idtr = Dtr { limit: 0, base: 0 };
        core::arch::asm!("sgdt [{0}]", in(reg) &mut gdtr, options(nostack));
        core::arch::asm!("sidt [{0}]", in(reg) &mut idtr, options(nostack));

        vmwrite(HOST_GDTR_BASE, gdtr.base)?;
        vmwrite(HOST_IDTR_BASE, idtr.base)?;
        vmwrite(HOST_FS_BASE, 0)?;
        vmwrite(HOST_IA32_SYSENTER_CS, 0)?;
        vmwrite(HOST_SYSENTER_ESP, 0)?;
        vmwrite(HOST_SYSENTER_EIP, 0)?;

        // Host GS base from MSR
        let host_gs_base: u64;
        core::arch::asm!(
            "rdmsr",
            in("ecx") 0xC000_0101u32, // MSR_GS_BASE
            out("eax") host_gs_base,
            out("edx") _,
            options(nomem, nostack),
        );
        vmwrite(HOST_GS_BASE, host_gs_base)?;

        // ── 2. Control Fields Setup ──────────────────────────────────────────
        let use_true = has_true_ctls();
        let pin_msr = if use_true { MSR_IA32_VMX_TRUE_PINBASED_CTLS } else { MSR_IA32_VMX_PINBASED_CTLS };
        let proc_msr = if use_true { MSR_IA32_VMX_TRUE_PROCBASED_CTLS } else { MSR_IA32_VMX_PROCBASED_CTLS };
        let exit_msr = if use_true { MSR_IA32_VMX_TRUE_EXIT_CTLS } else { MSR_IA32_VMX_EXIT_CTLS };
        let entry_msr = if use_true { MSR_IA32_VMX_TRUE_ENTRY_CTLS } else { MSR_IA32_VMX_ENTRY_CTLS };

        let pin_ctls = adjust_vmx_ctrl(PIN_EXTERNAL_INTERRUPT, pin_msr);
        vmwrite(PIN_BASED_VM_EXEC_CONTROL, pin_ctls as u64)?;

        let proc_ctls = adjust_vmx_ctrl(
            PROC_HLT_EXITING | PROC_UNCONDITIONAL_IO | PROC_ACTIVATE_SECONDARY_CONTROLS,
            proc_msr,
        );
        vmwrite(CPU_BASED_VM_EXEC_CONTROL, proc_ctls as u64)?;

        let sec_ctls = adjust_vmx_ctrl(
            SEC_ENABLE_EPT | SEC_ENABLE_RDTSCP | SEC_UNRESTRICTED_GUEST,
            MSR_IA32_VMX_PROCBASED_CTLS2,
        );
        vmwrite(SECONDARY_VM_EXEC_CONTROL, sec_ctls as u64)?;

        let exit_ctls = adjust_vmx_ctrl(
            EXIT_HOST_ADDR_SPACE_SIZE | EXIT_ACKNOWLEDGE_INTERRUPT,
            exit_msr,
        );
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
            (GUEST_DS_SELECTOR, GUEST_DS_BASE, GUEST_DS_LIMIT, GUEST_DS_AR_BYTES),
            (GUEST_ES_SELECTOR, GUEST_ES_BASE, GUEST_ES_LIMIT, GUEST_ES_AR_BYTES),
            (GUEST_SS_SELECTOR, GUEST_SS_BASE, GUEST_SS_LIMIT, GUEST_SS_AR_BYTES),
            (GUEST_FS_SELECTOR, GUEST_FS_BASE, GUEST_FS_LIMIT, GUEST_FS_AR_BYTES),
            (GUEST_GS_SELECTOR, GUEST_GS_BASE, GUEST_GS_LIMIT, GUEST_GS_AR_BYTES),
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

        // PVH ABI: %ebx points to hvm_start_info
        self.gpr[3] = hvm_info_gpa; // index 3 = RBX

        Ok(())
    }

    /// Run the guest vCPU until an exit requires hypervisor handling.
    pub fn run(&mut self) -> ViResult<ViVmExit> {
        loop {
            // Make sure our VMCS is active
            unsafe {
                vmptrld(self.vmcs_pa)?;
            }

            let status = unsafe { vmx_vmrun(self.gpr.as_mut_ptr(), self.launched as u64) };
            if status != 0 {
                return Ok(ViVmExit::Shutdown);
            }

            self.launched = true;

            let exit_reason = unsafe { (vmread(VM_EXIT_REASON)? & 0xFFFF) as u32 };
            let insn_len = unsafe { vmread(VM_EXIT_INSTRUCTION_LEN).unwrap_or(0) };
            let guest_rip = unsafe { vmread(GUEST_RIP)? };

            match exit_reason {
                EXIT_REASON_HLT => {
                    // Advance past HLT instruction
                    unsafe {
                        let _ = vmwrite(GUEST_RIP, guest_rip + insn_len);
                    }
                    return Ok(ViVmExit::Hlt);
                }
                EXIT_REASON_EXTERNAL_INTERRUPT => {
                    return Ok(ViVmExit::Preempted);
                }
                EXIT_REASON_IO_INSTRUCTION => {
                    let qual = unsafe { vmread(EXIT_QUALIFICATION)? };
                    let size = (qual & 0x7) as u8 + 1; // 0=1 byte, 1=2 bytes, 2=4 bytes
                    let is_in = (qual & (1 << 3)) != 0;
                    let port = ((qual >> 16) & 0xFFFF) as u16;

                    // Advance past IN/OUT
                    unsafe {
                        let _ = vmwrite(GUEST_RIP, guest_rip + insn_len);
                    }

                    if is_in {
                        return Ok(ViVmExit::PortIn { port, size });
                    } else {
                        let val = (self.gpr[0] & match size {
                            1 => 0xFF,
                            2 => 0xFFFF,
                            _ => 0xFFFF_FFFF,
                        }) as u32;
                        return Ok(ViVmExit::PortOut { port, size, val });
                    }
                }
                EXIT_REASON_EPT_VIOLATION => {
                    let gpa = unsafe { vmread(GUEST_PHYSICAL_ADDRESS)? };
                    let qual = unsafe { vmread(EXIT_QUALIFICATION)? };
                    let is_write = (qual & (1 << 1)) != 0;

                    // Advance RIP
                    unsafe {
                        let _ = vmwrite(GUEST_RIP, guest_rip + insn_len);
                    }

                    if is_write {
                        return Ok(ViVmExit::MmioWrite {
                            ipa: gpa,
                            size: 4,
                            val: self.gpr[0],
                        });
                    } else {
                        return Ok(ViVmExit::MmioRead {
                            ipa: gpa,
                            size: 4,
                            reg: 0,
                        });
                    }
                }
                EXIT_REASON_CPUID => {
                    // Emulate standard CPUID internally
                    let leaf = self.gpr[0] as u32; // EAX
                    let subleaf = self.gpr[1] as u32; // ECX
                    let res = core::arch::x86_64::__cpuid_count(leaf, subleaf);
                    self.gpr[0] = res.eax as u64;
                    self.gpr[3] = res.ebx as u64; // EBX
                    self.gpr[1] = res.ecx as u64; // ECX
                    self.gpr[2] = res.edx as u64; // EDX

                    // Clear hypervisor-incompatible bits if needed
                    if leaf == 1 {
                        // Clear x2APIC bit (ECX[21]) to encourage xAPIC
                        self.gpr[1] &= !(1 << 21);
                    }

                    unsafe {
                        let _ = vmwrite(GUEST_RIP, guest_rip + insn_len);
                    }
                    continue;
                }
                EXIT_REASON_CR_ACCESS => {
                    // CR0.PG flip for long mode
                    let qual = unsafe { vmread(EXIT_QUALIFICATION)? };
                    let cr_num = qual & 0xF;
                    let reg_idx = ((qual >> 8) & 0xF) as usize;
                    let reg_val = if reg_idx == 4 {
                        unsafe { vmread(GUEST_RSP)? }
                    } else if reg_idx < 16 {
                        self.gpr[reg_idx]
                    } else {
                        0
                    };

                    if cr_num == 0 {
                        // CR0 write: if turning on paging (bit 31), enable IA-32e mode in entry controls
                        if reg_val & (1 << 31) != 0 {
                            let entry_msr = if unsafe { has_true_ctls() } {
                                MSR_IA32_VMX_TRUE_ENTRY_CTLS
                            } else {
                                MSR_IA32_VMX_ENTRY_CTLS
                            };
                            let entry_ctls = unsafe {
                                adjust_vmx_ctrl(ENTRY_IA32E_MODE_GUEST, entry_msr)
                            };
                            unsafe {
                                let _ = vmwrite(VM_ENTRY_CONTROLS, entry_ctls as u64);
                                let _ = vmwrite(GUEST_CR0, reg_val);
                                let _ = vmwrite(CR0_READ_SHADOW, reg_val);
                            }
                        }
                    }

                    unsafe {
                        let _ = vmwrite(GUEST_RIP, guest_rip + insn_len);
                    }
                    continue;
                }
                EXIT_REASON_TRIPLE_FAULT => {
                    return Ok(ViVmExit::Shutdown);
                }
                other => {
                    unsafe {
                        let _ = vmwrite(GUEST_RIP, guest_rip + insn_len);
                    }
                    return Ok(ViVmExit::Unknown {
                        ec: other,
                        iss: insn_len as u32,
                    });
                }
            }
        }
    }
}
