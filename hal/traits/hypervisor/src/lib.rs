#![no_std]
use types::*;

/// VM exit reason returned by `ViHypervisor::run_vcpu`.
///
/// Variants are stable across P01-P03; the binary layout (not `#[repr(C)]`) is
/// intentionally left non-ABI-frozen until P04 per Law 1 — callers must match
/// the VMM crate version at compile time.
#[derive(Debug, Clone, Copy)]
pub enum ViVmExit {
    MmioRead {
        ipa: u64,
        size: u8,
        reg: u8,
    },
    MmioWrite {
        ipa: u64,
        size: u8,
        val: u64,
    },
    Hvc {
        imm: u16,
        regs: [u64; 8],
    },
    Wfi,
    /// System-register access (EC=0x18) — timer register emulation (P05+).
    SysReg {
        op0: u8,
        op1: u8,
        crn: u8,
        crm: u8,
        op2: u8,
        rt: u8,
        is_write: bool,
    },
    // ── x86 (SVM/VT-x) exit variants (Tier 3b x86 VMM, P03) ─────────────────
    /// Guest executed `IN` from an I/O port (x86 IOIO exit).
    PortIn {
        port: u16,
        size: u8,
    },
    /// Guest executed `OUT` to an I/O port. `val` holds the low `size` bytes.
    PortOut {
        port: u16,
        size: u8,
        val: u32,
    },
    /// Guest executed `HLT`.
    Hlt,
    /// Guest RDMSR/WRMSR (x86). `index` = ECX; `value` = EDX:EAX on write.
    Msr {
        index: u32,
        is_write: bool,
        value: u64,
    },
    Preempted,
    Shutdown,
    Unknown {
        ec: u32,
        iss: u32,
    },
}

/// Hypervisor trait — implemented by the architecture-specific VMM.
///
/// # Law 1 note
/// This trait and its associated types constitute public API.  Changing
/// method signatures requires 2× user confirmation once P04 freezes the ABI.
pub trait ViHypervisor {
    type Vm;
    type Vcpu;
    type Stage2Table;

    fn create_vm(&self) -> ViResult<Self::Vm>;
    fn create_vcpu(&self, vm: &mut Self::Vm) -> ViResult<Self::Vcpu>;
    fn map_guest(
        &self,
        table: &mut Self::Stage2Table,
        ipa: u64,
        hpa: u64,
        pages: usize,
        writable: bool,
    ) -> ViResult<()>;
    fn run_vcpu(&self, vcpu: &mut Self::Vcpu) -> ViResult<ViVmExit>;
    fn inject_irq(&self, vcpu: &mut Self::Vcpu, intid: u32) -> ViResult<()>;
}

/// Stub Vm type for the P01 skeleton — replaced with a real impl in P03.
pub struct ViVmStub;
/// Stub Vcpu type for the P01 skeleton — replaced with a real impl in P03.
pub struct ViVcpuStub;
/// Stub Stage-2 page table type — replaced with a real impl in P03.
pub struct ViStage2TableStub;
