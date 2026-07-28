// SPDX-License-Identifier: MPL-2.0
//! ARM64 EL2 VMM syscall ABI — stable kernel↔cell contract.
//!
//! ⚠️ **Law 1**: this file is part of the stable ABI between kernel and Cells.
//! Any changes require 2× user confirmation.  `VERSION = 2` (Tier 3b x86 P04
//! appended discriminants 8-11).  To add new exit types, add variants at new
//! explicit discriminant values only — never change existing discriminants,
//! field names, or field types.
//!
//! # Confidential-computing neutrality (frozen invariants — `review-cc-neutral-abi-freeze.md`)
//! The VERSION=2 freeze must not preclude TDX / SEV-SNP / ARM-CCA guests later:
//! - **I1 (size envelope is the freeze boundary):** `size_of::<ViVmExit>()` is
//!   pinned by the `Hvc { regs: [u64; 8] }` variant (~80 B) and asserted below.
//!   A future CC exit MUST carry a *shared-region reference* (e.g. a GHCB GPA +
//!   metadata), NEVER an inline guest register file — a TDX `TDG.VP.VMCALL`
//!   register dump (~13 GPRs, ~104 B) would overflow the envelope and break every
//!   VERSION=2 cell's `validate_user_buf`.
//! - **I2 (field provenance):** every field is a value the guest EXPLICITLY
//!   delivered (ISV=1 syndrome / IOIO qualifier / GHCB / TDVMCALL). `Hvc.regs`
//!   are published hypercall args only; no variant carries guest RIP or raw
//!   instruction bytes.
//! - **I3 (append-only for CC):** a CC attested-launch / sysreg path is always a
//!   NEW variant at discriminant 12+, never a reshape of PortIn/PortOut/Msr.

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
    MmioRead { ipa: u64, size: u8, reg: u8 } = 0,
    /// Stage-2 data-abort (write) — unmapped MMIO IPA; ISV=1 guaranteed.
    MmioWrite { ipa: u64, size: u8, val: u64 } = 1,
    /// HVC instruction — covers PSCI calls and general hypercall ABI.
    Hvc { imm: u16, regs: [u64; 8] } = 2,
    /// WFI instruction — guest idle; hypervisor may inject a virtual IRQ.
    Wfi = 3,
    /// System-register access (EC=0x18) — timer register emulation (P05+).
    SysReg {
        op0: u8,
        op1: u8,
        crn: u8,
        crm: u8,
        op2: u8,
        rt: u8,
        is_write: bool,
    } = 4,
    /// `budget_ns` budget expired — no guest fault; re-enter after servicing IPC.
    Preempted = 5,
    /// Guest requested shutdown — PSCI SYSTEM_OFF / CPU_OFF (P05+).
    Shutdown = 6,
    /// Unrecognized exception class — includes S1PTW=1 stage-1 walk faults.
    /// Treat as fatal guest fault: log `ec`/`iss` and halt the VM.
    Unknown { ec: u32, iss: u32 } = 7,

    // ── x86 (SVM/VT-x) exits — appended in Tier 3b P04, VERSION 2 ────────────
    /// x86 `IN` from an I/O port. `reg` is reserved (guest `IN` always targets
    /// (E)AX); kept for symmetry with `MmioRead`.
    PortIn { port: u16, size: u8, reg: u8 } = 8,
    /// x86 `OUT` to an I/O port. `val` holds the low `size` bytes written.
    PortOut { port: u16, size: u8, val: u32 } = 9,
    /// x86 `HLT` — guest idle; the hypervisor may inject an IRQ (P05).
    Hlt = 10,
    /// x86 RDMSR/WRMSR. `index` = ECX; `val` = EDX:EAX on a write.
    Msr {
        index: u32,
        is_write: bool,
        val: u64,
    } = 11,
}

impl ViVmExit {
    /// ABI version — increment when adding new discriminant values.
    pub const VERSION: u32 = 2;
}

// I1: the x86 variants (≤ 16 B payload) must NOT grow the enum past the
// `Hvc { regs: [u64; 8] }` envelope, or `validate_user_buf(size_of::<ViVmExit>())`
// in the run_vcpu syscall path breaks for every existing cell. Pin it.
const _: () = assert!(core::mem::size_of::<ViVmExit>() == 80);
