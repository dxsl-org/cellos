// SPDX-License-Identifier: MPL-2.0
//! ARM64 EL2 VMM syscall ABI — stable kernel↔cell contract.
//!
//! ⚠️ **Law 1**: this file is part of the stable ABI between kernel and Cells.
//! Any changes require 2× user confirmation.  `VERSION = 1` is frozen at Phase 04.
//! To add new exit types, add variants at new explicit discriminant values only —
//! never change existing discriminants, field names, or field types.

/// VM exit reason written by `sys_run_vcpu` into the caller-provided out-param.
///
/// `#[repr(C, u8)]` guarantees a stable ABI: the `u8` discriminant precedes each
/// variant's payload in memory, and the total size equals the largest variant
/// padded to alignment.  The kernel writes via `*mut ViVmExit` (SAS: kernel and
/// cell share the same virtual address space, so the pointer is valid in both).
///
/// **Frozen at VERSION 1.** Never modify existing variant fields.
#[repr(C, u8)]
#[derive(Debug, Clone, Copy)]
pub enum ViVmExit {
    /// Stage-2 data-abort (read) — unmapped MMIO IPA; ISV=1 guaranteed.
    MmioRead  { ipa: u64, size: u8, reg: u8 }                                   = 0,
    /// Stage-2 data-abort (write) — unmapped MMIO IPA; ISV=1 guaranteed.
    MmioWrite { ipa: u64, size: u8, val: u64 }                                  = 1,
    /// HVC instruction — covers PSCI calls and general hypercall ABI.
    Hvc       { imm: u16, regs: [u64; 8] }                                      = 2,
    /// WFI instruction — guest idle; hypervisor may inject a virtual IRQ.
    Wfi                                                                          = 3,
    /// System-register access (EC=0x18) — timer register emulation (P05+).
    SysReg    { op0: u8, op1: u8, crn: u8, crm: u8, op2: u8, rt: u8, is_write: bool } = 4,
    /// `budget_ns` budget expired — no guest fault; re-enter after servicing IPC.
    Preempted                                                                    = 5,
    /// Guest requested shutdown — PSCI SYSTEM_OFF / CPU_OFF (P05+).
    Shutdown                                                                     = 6,
    /// Unrecognized exception class — includes S1PTW=1 stage-1 walk faults.
    /// Treat as fatal guest fault: log `ec`/`iss` and halt the VM.
    Unknown   { ec: u32, iss: u32 }                                             = 7,
}

impl ViVmExit {
    /// ABI version — increment when adding new discriminant values.
    pub const VERSION: u32 = 1;
}
