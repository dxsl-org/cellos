// SPDX-License-Identifier: MIT
//! Intel VT-x VMCS (Virtual Machine Control Structure) definitions and operations.
//!
//! A VMCS is a 4 KiB frame (4 KiB-aligned) holding host state, guest state,
//! execution controls, and exit information for one virtual processor.
//! Unlike AMD VMCB, VMCS fields are not memory-mapped at fixed byte offsets;
//! they are accessed exclusively through `vmread` and `vmwrite` CPU instructions
//! using 32-bit field encodings.
//!
//! Specification: Intel SDM Volume 3C, Chapters 24, 25, 26, 27 and Appendix B.

use types::{ViError, ViResult};

// ── VMX Instructions ─────────────────────────────────────────────────────────

/// Execute `vmclear` to flush VMCS data to memory and set its launch state to Clear.
///
/// # Safety
/// `vmcs_pa` must be a valid, 4 KiB-aligned physical address of a VMCS region.
#[inline]
pub unsafe fn vmclear(vmcs_pa: u64) -> ViResult<()> {
    let rflags: u64;
    core::arch::asm!(
        "vmclear [{pa}]",
        "pushfq",
        "pop {rf}",
        pa = in(reg) &vmcs_pa,
        rf = out(reg) rflags,
        options(nostack),
    );
    if rflags & 0x41 != 0 {
        Err(ViError::IO)
    } else {
        Ok(())
    }
}

/// Execute `vmptrld` to mark the VMCS at `vmcs_pa` as the active VMCS on the current CPU.
///
/// # Safety
/// `vmcs_pa` must be a valid, 4 KiB-aligned physical address of a VMCS region
/// initialized with the correct revision ID.
#[inline]
pub unsafe fn vmptrld(vmcs_pa: u64) -> ViResult<()> {
    let rflags: u64;
    core::arch::asm!(
        "vmptrld [{pa}]",
        "pushfq",
        "pop {rf}",
        pa = in(reg) &vmcs_pa,
        rf = out(reg) rflags,
        options(nostack),
    );
    if rflags & 0x41 != 0 {
        Err(ViError::IO)
    } else {
        Ok(())
    }
}

/// Read a 64-bit field from the current active VMCS using `vmread`.
///
/// # Safety
/// A valid VMCS must be active on the current CPU (`vmptrld`).
#[inline]
pub unsafe fn vmread(field: u32) -> ViResult<u64> {
    let val: u64;
    let rflags: u64;
    core::arch::asm!(
        "vmread {val}, {field}",
        "pushfq",
        "pop {rf}",
        field = in(reg) field as u64,
        val = out(reg) val,
        rf = out(reg) rflags,
        options(nostack),
    );
    if rflags & 0x41 != 0 {
        Err(ViError::IO)
    } else {
        Ok(val)
    }
}

/// Write a 64-bit value to a field in the current active VMCS using `vmwrite`.
///
/// # Safety
/// A valid VMCS must be active on the current CPU (`vmptrld`).
#[inline]
pub unsafe fn vmwrite(field: u32, val: u64) -> ViResult<()> {
    let rflags: u64;
    core::arch::asm!(
        "vmwrite {field}, {val}",
        "pushfq",
        "pop {rf}",
        field = in(reg) field as u64,
        val = in(reg) val,
        rf = out(reg) rflags,
        options(nostack),
    );
    if rflags & 0x41 != 0 {
        Err(ViError::IO)
    } else {
        Ok(())
    }
}

// ── VMX Control MSRs ─────────────────────────────────────────────────────────

pub const MSR_IA32_VMX_BASIC: u32 = 0x480;
pub const MSR_IA32_VMX_PINBASED_CTLS: u32 = 0x481;
pub const MSR_IA32_VMX_PROCBASED_CTLS: u32 = 0x482;
pub const MSR_IA32_VMX_EXIT_CTLS: u32 = 0x483;
pub const MSR_IA32_VMX_ENTRY_CTLS: u32 = 0x484;
pub const MSR_IA32_VMX_MISC: u32 = 0x485;
pub const MSR_IA32_VMX_CR0_FIXED0: u32 = 0x486;
pub const MSR_IA32_VMX_CR0_FIXED1: u32 = 0x487;
pub const MSR_IA32_VMX_CR4_FIXED0: u32 = 0x488;
pub const MSR_IA32_VMX_CR4_FIXED1: u32 = 0x489;
pub const MSR_IA32_VMX_PROCBASED_CTLS2: u32 = 0x48B;
pub const MSR_IA32_VMX_EPT_VPID_CAP: u32 = 0x48C;
pub const MSR_IA32_VMX_TRUE_PINBASED_CTLS: u32 = 0x48D;
pub const MSR_IA32_VMX_TRUE_PROCBASED_CTLS: u32 = 0x48E;
pub const MSR_IA32_VMX_TRUE_EXIT_CTLS: u32 = 0x48F;
pub const MSR_IA32_VMX_TRUE_ENTRY_CTLS: u32 = 0x490;

/// Adjust a requested 32-bit VMX control bitmask against the CPU's capability MSR.
///
/// Intel SDM 31.5.1: Bits that must be 1 (allowed 0 is 0) are forced on;
/// bits that cannot be 1 (allowed 1 is 0) are forced off.
#[inline]
pub unsafe fn adjust_vmx_ctrl(desired: u32, msr: u32) -> u32 {
    let mut lo: u32;
    let mut hi: u32;
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") lo,
        out("edx") hi,
        options(nomem, nostack),
    );
    (desired | lo) & hi
}

/// Check whether the CPU supports the "TRUE" capability MSRs (IA32_VMX_BASIC[55] == 1).
#[inline]
pub unsafe fn has_true_ctls() -> bool {
    let mut lo: u32;
    let mut hi: u32;
    core::arch::asm!(
        "rdmsr",
        in("ecx") MSR_IA32_VMX_BASIC,
        out("eax") lo,
        out("edx") hi,
        options(nomem, nostack),
    );
    let _ = lo;
    (hi & (1 << (55 - 32))) != 0
}

// ── VMCS Field Encodings (Intel SDM Appendix B) ──────────────────────────────

// 16-bit Guest State Fields
pub const GUEST_ES_SELECTOR: u32 = 0x0800;
pub const GUEST_CS_SELECTOR: u32 = 0x0802;
pub const GUEST_SS_SELECTOR: u32 = 0x0804;
pub const GUEST_DS_SELECTOR: u32 = 0x0806;
pub const GUEST_FS_SELECTOR: u32 = 0x0808;
pub const GUEST_GS_SELECTOR: u32 = 0x080A;
pub const GUEST_LDTR_SELECTOR: u32 = 0x080C;
pub const GUEST_TR_SELECTOR: u32 = 0x080E;

// 16-bit Host State Fields
pub const HOST_ES_SELECTOR: u32 = 0x0C00;
pub const HOST_CS_SELECTOR: u32 = 0x0C02;
pub const HOST_SS_SELECTOR: u32 = 0x0C04;
pub const HOST_DS_SELECTOR: u32 = 0x0C06;
pub const HOST_FS_SELECTOR: u32 = 0x0C08;
pub const HOST_GS_SELECTOR: u32 = 0x0C0A;
pub const HOST_TR_SELECTOR: u32 = 0x0C0C;

// 64-bit Control Fields
pub const IO_BITMAP_A: u32 = 0x2000;
pub const IO_BITMAP_B: u32 = 0x2002;
pub const MSR_BITMAP: u32 = 0x2004;
pub const VM_EXIT_MSR_STORE_ADDR: u32 = 0x2006;
pub const VM_EXIT_MSR_LOAD_ADDR: u32 = 0x2008;
pub const VM_ENTRY_MSR_LOAD_ADDR: u32 = 0x200A;
pub const EXECUTIVE_VMCS_POINTER: u32 = 0x200C;
pub const TSC_OFFSET: u32 = 0x2010;
pub const VIRTUAL_APIC_PAGE_ADDR: u32 = 0x2012;
pub const APIC_ACCESS_ADDR: u32 = 0x2014;
pub const EPT_POINTER: u32 = 0x201A;

// 64-bit Read-Only Data Fields
pub const GUEST_PHYSICAL_ADDRESS: u32 = 0x2400;

// 64-bit Guest State Fields
pub const VMCS_LINK_POINTER: u32 = 0x2800;
pub const GUEST_IA32_DEBUGCTL: u32 = 0x2802;
pub const GUEST_IA32_PAT: u32 = 0x2804;
pub const GUEST_IA32_EFER: u32 = 0x2806;
pub const GUEST_IA32_PERF_GLOBAL_CTRL: u32 = 0x2808;

// 64-bit Host State Fields
pub const HOST_IA32_PAT: u32 = 0x2C00;
pub const HOST_IA32_EFER: u32 = 0x2C02;
pub const HOST_IA32_PERF_GLOBAL_CTRL: u32 = 0x2C04;

// 32-bit Control Fields
pub const PIN_BASED_VM_EXEC_CONTROL: u32 = 0x4000;
pub const CPU_BASED_VM_EXEC_CONTROL: u32 = 0x4002;
pub const EXCEPTION_BITMAP: u32 = 0x4004;
pub const PAGE_FAULT_ERROR_CODE_MASK: u32 = 0x4006;
pub const PAGE_FAULT_ERROR_CODE_MATCH: u32 = 0x4008;
pub const CR3_TARGET_COUNT: u32 = 0x400A;
pub const VM_EXIT_CONTROLS: u32 = 0x400C;
pub const VM_EXIT_MSR_STORE_COUNT: u32 = 0x400E;
pub const VM_EXIT_MSR_LOAD_COUNT: u32 = 0x4010;
pub const VM_ENTRY_CONTROLS: u32 = 0x4012;
pub const VM_ENTRY_MSR_LOAD_COUNT: u32 = 0x4014;
pub const VM_ENTRY_INTR_INFO_FIELD: u32 = 0x4016;
pub const VM_ENTRY_EXCEPTION_ERROR_CODE: u32 = 0x4018;
pub const VM_ENTRY_INSTRUCTION_LEN: u32 = 0x401A;
pub const TPR_THRESHOLD: u32 = 0x401C;
pub const SECONDARY_VM_EXEC_CONTROL: u32 = 0x401E;

// 32-bit Read-Only Data Fields
pub const VM_INSTRUCTION_ERROR: u32 = 0x4400;
pub const VM_EXIT_REASON: u32 = 0x4402;
pub const VM_EXIT_INTR_INFO: u32 = 0x4404;
pub const VM_EXIT_INTR_ERROR_CODE: u32 = 0x4406;
pub const IDT_VECTORING_INFO_FIELD: u32 = 0x4408;
pub const IDT_VECTORING_ERROR_CODE: u32 = 0x440A;
pub const VM_EXIT_INSTRUCTION_LEN: u32 = 0x440C;
pub const VMX_INSTRUCTION_INFO: u32 = 0x440E;

// 32-bit Guest State Fields
pub const GUEST_ES_LIMIT: u32 = 0x4800;
pub const GUEST_CS_LIMIT: u32 = 0x4802;
pub const GUEST_SS_LIMIT: u32 = 0x4804;
pub const GUEST_DS_LIMIT: u32 = 0x4806;
pub const GUEST_FS_LIMIT: u32 = 0x4808;
pub const GUEST_GS_LIMIT: u32 = 0x480A;
pub const GUEST_LDTR_LIMIT: u32 = 0x480C;
pub const GUEST_TR_LIMIT: u32 = 0x480E;
pub const GUEST_GDTR_LIMIT: u32 = 0x4810;
pub const GUEST_IDTR_LIMIT: u32 = 0x4812;
pub const GUEST_ES_AR_BYTES: u32 = 0x4814;
pub const GUEST_CS_AR_BYTES: u32 = 0x4816;
pub const GUEST_SS_AR_BYTES: u32 = 0x4818;
pub const GUEST_DS_AR_BYTES: u32 = 0x481A;
pub const GUEST_FS_AR_BYTES: u32 = 0x481C;
pub const GUEST_GS_AR_BYTES: u32 = 0x481E;
pub const GUEST_LDTR_AR_BYTES: u32 = 0x4820;
pub const GUEST_TR_AR_BYTES: u32 = 0x4822;
pub const GUEST_INTERRUPTIBILITY_INFO: u32 = 0x4824;
pub const GUEST_ACTIVITY_STATE: u32 = 0x4826;
pub const GUEST_SYSENTER_CS: u32 = 0x482A;

// 32-bit Host State Fields
pub const HOST_IA32_SYSENTER_CS: u32 = 0x4C00;

// Natural-Width Control Fields
pub const CR0_GUEST_HOST_MASK: u32 = 0x6000;
pub const CR4_GUEST_HOST_MASK: u32 = 0x6002;
pub const CR0_READ_SHADOW: u32 = 0x6004;
pub const CR4_READ_SHADOW: u32 = 0x6006;

// Natural-Width Read-Only Data Fields
pub const EXIT_QUALIFICATION: u32 = 0x6400;
pub const IO_RCX: u32 = 0x6402;
pub const IO_RSI: u32 = 0x6404;
pub const IO_RDI: u32 = 0x6406;
pub const IO_RIP: u32 = 0x6408;
pub const GUEST_LINEAR_ADDRESS: u32 = 0x640A;

// Natural-Width Guest State Fields
pub const GUEST_CR0: u32 = 0x6800;
pub const GUEST_CR3: u32 = 0x6802;
pub const GUEST_CR4: u32 = 0x6804;
pub const GUEST_ES_BASE: u32 = 0x6806;
pub const GUEST_CS_BASE: u32 = 0x6808;
pub const GUEST_SS_BASE: u32 = 0x680A;
pub const GUEST_DS_BASE: u32 = 0x680C;
pub const GUEST_FS_BASE: u32 = 0x680E;
pub const GUEST_GS_BASE: u32 = 0x6810;
pub const GUEST_LDTR_BASE: u32 = 0x6812;
pub const GUEST_TR_BASE: u32 = 0x6814;
pub const GUEST_GDTR_BASE: u32 = 0x6816;
pub const GUEST_IDTR_BASE: u32 = 0x6818;
pub const GUEST_DR7: u32 = 0x681A;
pub const GUEST_RSP: u32 = 0x681C;
pub const GUEST_RIP: u32 = 0x681E;
pub const GUEST_RFLAGS: u32 = 0x6820;
pub const GUEST_PENDING_DBG_EXCEPTIONS: u32 = 0x6822;
pub const GUEST_SYSENTER_ESP: u32 = 0x6824;
pub const GUEST_SYSENTER_EIP: u32 = 0x6826;

// Natural-Width Host State Fields
pub const HOST_CR0: u32 = 0x6C00;
pub const HOST_CR3: u32 = 0x6C02;
pub const HOST_CR4: u32 = 0x6C04;
pub const HOST_FS_BASE: u32 = 0x6C06;
pub const HOST_GS_BASE: u32 = 0x6C08;
pub const HOST_TR_BASE: u32 = 0x6C0A;
pub const HOST_GDTR_BASE: u32 = 0x6C0C;
pub const HOST_IDTR_BASE: u32 = 0x6C0E;
pub const HOST_SYSENTER_ESP: u32 = 0x6C10;
pub const HOST_SYSENTER_EIP: u32 = 0x6C12;
pub const HOST_RSP: u32 = 0x6C14;
pub const HOST_RIP: u32 = 0x6C16;

// ── Control Bits ─────────────────────────────────────────────────────────────

// Pin-based controls
pub const PIN_EXTERNAL_INTERRUPT: u32 = 1 << 0;
pub const PIN_NMI_EXITING: u32 = 1 << 3;
pub const PIN_VIRTUAL_NMIS: u32 = 1 << 5;
pub const PIN_VMX_PREEMPTION_TIMER: u32 = 1 << 6;

// Primary Processor-based controls
pub const PROC_INTERRUPT_WINDOW: u32 = 1 << 2;
pub const PROC_USE_TSC_OFFSETTING: u32 = 1 << 3;
pub const PROC_HLT_EXITING: u32 = 1 << 7;
pub const PROC_INVLPG_EXITING: u32 = 1 << 9;
pub const PROC_MWAIT_EXITING: u32 = 1 << 10;
pub const PROC_RDPMC_EXITING: u32 = 1 << 11;
pub const PROC_RDTSC_EXITING: u32 = 1 << 12;
pub const PROC_CR3_LOAD_EXITING: u32 = 1 << 15;
pub const PROC_CR3_STORE_EXITING: u32 = 1 << 16;
pub const PROC_CR8_LOAD_EXITING: u32 = 1 << 19;
pub const PROC_CR8_STORE_EXITING: u32 = 1 << 20;
pub const PROC_USE_TPR_SHADOW: u32 = 1 << 21;
pub const PROC_MOV_DR_EXITING: u32 = 1 << 23;
pub const PROC_UNCONDITIONAL_IO: u32 = 1 << 24;
pub const PROC_USE_IO_BITMAPS: u32 = 1 << 25;
pub const PROC_MONITOR_TRAP_FLAG: u32 = 1 << 27;
pub const PROC_USE_MSR_BITMAPS: u32 = 1 << 28;
pub const PROC_MONITOR_EXITING: u32 = 1 << 29;
pub const PROC_PAUSE_EXITING: u32 = 1 << 30;
pub const PROC_ACTIVATE_SECONDARY_CONTROLS: u32 = 1 << 31;

// Secondary Processor-based controls
pub const SEC_VIRTUALIZE_APIC_ACCESSES: u32 = 1 << 0;
pub const SEC_ENABLE_EPT: u32 = 1 << 1;
pub const SEC_DESCRIPTOR_TABLE_EXITING: u32 = 1 << 2;
pub const SEC_ENABLE_RDTSCP: u32 = 1 << 3;
pub const SEC_VIRTUALIZE_X2APIC: u32 = 1 << 4;
pub const SEC_ENABLE_VPID: u32 = 1 << 5;
pub const SEC_WBINVD_EXITING: u32 = 1 << 6;
pub const SEC_UNRESTRICTED_GUEST: u32 = 1 << 7;
pub const SEC_APIC_REGISTER_VIRTUALIZATION: u32 = 1 << 8;
pub const SEC_VIRTUAL_INTERRUPT_DELIVERY: u32 = 1 << 9;
pub const SEC_PAUSE_LOOP_EXITING: u32 = 1 << 10;
pub const SEC_RDRAND_EXITING: u32 = 1 << 11;
pub const SEC_ENABLE_INVPCID: u32 = 1 << 12;
pub const SEC_ENABLE_VMFUNC: u32 = 1 << 13;

// VM-exit controls
pub const EXIT_SAVE_DEBUG_CONTROLS: u32 = 1 << 2;
pub const EXIT_HOST_ADDR_SPACE_SIZE: u32 = 1 << 9; // 1 = 64-bit host
pub const EXIT_LOAD_IA32_PERF_GLOBAL_CTRL: u32 = 1 << 12;
pub const EXIT_ACKNOWLEDGE_INTERRUPT: u32 = 1 << 15;
pub const EXIT_SAVE_IA32_PAT: u32 = 1 << 18;
pub const EXIT_LOAD_IA32_PAT: u32 = 1 << 19;
pub const EXIT_SAVE_IA32_EFER: u32 = 1 << 20;
pub const EXIT_LOAD_IA32_EFER: u32 = 1 << 21;
pub const EXIT_SAVE_VMX_PREEMPTION_TIMER: u32 = 1 << 22;

// VM-entry controls
pub const ENTRY_LOAD_DEBUG_CONTROLS: u32 = 1 << 2;
pub const ENTRY_IA32E_MODE_GUEST: u32 = 1 << 9; // 1 = 64-bit guest
pub const ENTRY_LOAD_IA32_PERF_GLOBAL_CTRL: u32 = 1 << 13;
pub const ENTRY_LOAD_IA32_PAT: u32 = 1 << 14;
pub const ENTRY_LOAD_IA32_EFER: u32 = 1 << 15;

// Exit Reasons (Intel SDM Appendix C)
pub const EXIT_REASON_EXCEPTION_NMI: u32 = 0;
pub const EXIT_REASON_EXTERNAL_INTERRUPT: u32 = 1;
pub const EXIT_REASON_TRIPLE_FAULT: u32 = 2;
pub const EXIT_REASON_INIT: u32 = 3;
pub const EXIT_REASON_SIPI: u32 = 4;
pub const EXIT_REASON_INTERRUPT_WINDOW: u32 = 7;
pub const EXIT_REASON_NMI_WINDOW: u32 = 8;
pub const EXIT_REASON_TASK_SWITCH: u32 = 9;
pub const EXIT_REASON_CPUID: u32 = 10;
pub const EXIT_REASON_GETSEC: u32 = 11;
pub const EXIT_REASON_HLT: u32 = 12;
pub const EXIT_REASON_INVD: u32 = 13;
pub const EXIT_REASON_INVLPG: u32 = 14;
pub const EXIT_REASON_RDPMC: u32 = 15;
pub const EXIT_REASON_RDTSC: u32 = 16;
pub const EXIT_REASON_RSM: u32 = 17;
pub const EXIT_REASON_VMCALL: u32 = 18;
pub const EXIT_REASON_VMCLEAR: u32 = 19;
pub const EXIT_REASON_VMLAUNCH: u32 = 20;
pub const EXIT_REASON_VMPTRLD: u32 = 21;
pub const EXIT_REASON_VMPTRST: u32 = 22;
pub const EXIT_REASON_VMREAD: u32 = 23;
pub const EXIT_REASON_VMRESUME: u32 = 24;
pub const EXIT_REASON_VMWRITE: u32 = 25;
pub const EXIT_REASON_VMXOFF: u32 = 26;
pub const EXIT_REASON_VMXON: u32 = 27;
pub const EXIT_REASON_CR_ACCESS: u32 = 28;
pub const EXIT_REASON_DR_ACCESS: u32 = 29;
pub const EXIT_REASON_IO_INSTRUCTION: u32 = 30;
pub const EXIT_REASON_MSR_READ: u32 = 31;
pub const EXIT_REASON_MSR_WRITE: u32 = 32;
pub const EXIT_REASON_INVALID_GUEST_STATE: u32 = 33;
pub const EXIT_REASON_MSR_LOAD_FAIL: u32 = 34;
pub const EXIT_REASON_MWAIT_INSTRUCTION: u32 = 36;
pub const EXIT_REASON_MONITOR_TRAP_FLAG: u32 = 37;
pub const EXIT_REASON_MONITOR_INSTRUCTION: u32 = 39;
pub const EXIT_REASON_PAUSE_INSTRUCTION: u32 = 40;
pub const EXIT_REASON_MCE_DURING_VMENTRY: u32 = 41;
pub const EXIT_REASON_TPR_BELOW_THRESHOLD: u32 = 43;
pub const EXIT_REASON_APIC_ACCESS: u32 = 44;
pub const EXIT_REASON_EOI_INDUCED: u32 = 45;
pub const EXIT_REASON_GDTR_IDTR_ACCESS: u32 = 46;
pub const EXIT_REASON_LDTR_TR_ACCESS: u32 = 47;
pub const EXIT_REASON_EPT_VIOLATION: u32 = 48;
pub const EXIT_REASON_EPT_MISCONFIG: u32 = 49;
pub const EXIT_REASON_INVEPT: u32 = 50;
pub const EXIT_REASON_RDTSCP: u32 = 51;
pub const EXIT_REASON_VMX_PREEMPTION_TIMER: u32 = 52;
pub const EXIT_REASON_INVVPID: u32 = 53;
pub const EXIT_REASON_WBINVD: u32 = 54;
pub const EXIT_REASON_XSETBV: u32 = 55;
pub const EXIT_REASON_APIC_WRITE: u32 = 56;
pub const EXIT_REASON_RDRAND: u32 = 57;
pub const EXIT_REASON_INVPCID: u32 = 58;
pub const EXIT_REASON_VMFUNC: u32 = 59;
pub const EXIT_REASON_ENCLS: u32 = 60;
pub const EXIT_REASON_RDSEED: u32 = 61;
pub const EXIT_REASON_PML_FULL: u32 = 62;
pub const EXIT_REASON_XSAVES: u32 = 63;
pub const EXIT_REASON_XRSTORS: u32 = 64;
