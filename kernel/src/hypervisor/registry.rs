//! VM registry — per-owner RAII store for Stage-2 tables, guest RAM, and vCPUs.
//!
//! A VM belongs to the Cell incarnation that created it ([`VmKey`]), never to
//! a tid: every operation names that incarnation, and the incarnation's VMs
//! are reaped together when its root retires ([`reap_vms_for`]).
//!
//! # Lock order
//! `VM_REGISTRY` → `FRAME_ALLOCATOR` (same order as grant reaper; never reverse).
//!
//! # Non-aarch64
//! All public functions return `Err(ViError::NotSupported)` so the compiler
//! produces a complete match for the hypervisor Syscall arms on every target.

use crate::memory::cell_quota::{Charge, QuotaExceeded};
use core::sync::atomic::{AtomicUsize, Ordering};
use types::{ViError, ViResult};

/// Byte images of the records the hypervisor syscalls copy to the cell.
#[path = "vmm_wire.rs"]
pub(crate) mod vmm_wire;

/// A VM's registry key: its owner incarnation and an id never handed out
/// twice in a boot, so a key can never name another incarnation's VM or a
/// VM that was reaped.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct VmKey {
    owner: (u64, u64),
    vm_id: usize,
}

// reason: the targets without a hypervisor (riscv64, legacy) register no VM.
#[allow(dead_code)]
impl VmKey {
    pub(crate) fn new(owner: Charge, vm_id: usize) -> Self {
        Self {
            owner: (owner.cell().0, owner.generation()),
            vm_id,
        }
    }

    pub(crate) fn owned_by(self, owner: Charge) -> bool {
        self.owner == (owner.cell().0, owner.generation())
    }

    // reason: only the p01-trace root walk reads the raw parts.
    #[allow(dead_code)]
    pub(crate) fn parts(self) -> (u64, usize) {
        (self.owner.0, self.vm_id)
    }
}

/// The next VM id. Monotonic for the boot: an id is never reused, so a stale
/// id held by a cell names nothing.
static NEXT_VM_ID: AtomicUsize = AtomicUsize::new(1);

/// Mint the id of a VM about to be registered.
// reason: the targets without a hypervisor (riscv64, legacy) register no VM.
#[allow(dead_code)]
pub(crate) fn next_vm_id() -> usize {
    NEXT_VM_ID.fetch_add(1, Ordering::Relaxed)
}

/// Why `create_vm` refused. The `CreateVm` syscall returns `OutOfMemory` for
/// [`VmCreateError::Budget`] only; every other refusal stays `NotSupported`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VmCreateError {
    /// The owner's guest budget cannot cover the VM. Nothing was allocated
    /// and nothing stays charged.
    Budget,
    /// Any other refusal: no hypervisor, a failed root, carve or map, no
    /// VMID. What it allocated is freed and its charge refunded.
    Refused(ViError),
}

impl From<ViError> for VmCreateError {
    fn from(error: ViError) -> Self {
        Self::Refused(error)
    }
}

impl From<QuotaExceeded> for VmCreateError {
    fn from(_: QuotaExceeded) -> Self {
        Self::Budget
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Allocate guest RAM + Stage-2 table for `owner`; return opaque `vm_id`. The
/// owner's guest budget is charged before anything else, for the most frames
/// the VM can take (`arch::vm::guest_bytes`); a refusal is
/// `Err(VmCreateError::Budget)` with nothing allocated.
pub(crate) fn create_vm(owner: Charge, guest_pages: usize) -> Result<usize, VmCreateError> {
    crate::arch::vm::create_vm(owner, guest_pages)
}

/// Create a vCPU in `vm_id` with initial PC `entry_pc`; return `vcpu_id` (1-based).
///
/// Under `test-hooks`, writes a P04 HVC smoke blob (`MOVZ X0,#42; HVC #0; B .`)
/// to the page containing `entry_pc` so the test cell does not need userspace
/// memory access to guest RAM.
pub(crate) fn create_vcpu(owner: Charge, vm_id: usize, entry_pc: u64) -> ViResult<usize> {
    crate::arch::vm::create_vcpu(owner, vm_id, entry_pc)
}

/// Map guest IPA range in `vm_id`'s Stage-2.
pub(crate) fn map_guest_memory(
    owner: Charge,
    vm_id: usize,
    ipa: u64,
    size: usize,
    writable: bool,
) -> ViResult<()> {
    crate::arch::vm::map_guest_memory(owner, vm_id, ipa, size, writable)
}

/// The guest-RAM window of `vm_id`: its first guest-physical address and its
/// size in bytes. The syscall layer bounds every guest-memory request with it
/// before staging any data.
pub(crate) fn guest_window(owner: Charge, vm_id: usize) -> ViResult<(u64, usize)> {
    crate::arch::vm::guest_window(owner, vm_id)
}

/// World-switch into the vCPU and return the exit, converted to the API ABI.
/// The syscall layer copies it to the cell; this never touches cell memory.
pub(crate) fn run_vcpu(
    owner: Charge,
    vm_id: usize,
    vcpu_id: usize,
    _budget_ns: u64,
) -> ViResult<abi::hypervisor::ViVmExit> {
    crate::arch::vm::run_vcpu(owner, vm_id, vcpu_id, _budget_ns)
}

/// Modes 0/1 read/write 32 guest GP-register words. On Pi mode 2 reads a
/// 32-word timer snapshot (CNTV_CTL_EL0, CNTV_CVAL_EL0, then thirty zeros).
///
/// `regs` is a kernel copy of the cell's buffer: read modes fill it, mode 1
/// consumes it. Words a backend does not model are left as they were.
pub(crate) fn vcpu_regs(
    owner: Charge,
    vm_id: usize,
    vcpu_id: usize,
    regs: &mut [u64; 32],
    mode: usize,
) -> ViResult<usize> {
    crate::arch::vm::vcpu_regs(owner, vm_id, vcpu_id, regs, mode)
}

/// Copy `src` into guest physical RAM at `gpa`.
///
/// `src` is kernel memory (the syscall layer stages cell data through the
/// caller's copy view); the whole `[gpa, gpa + len)` range must lie inside the
/// guest-RAM window or nothing is written.
pub(crate) fn write_guest_memory(
    owner: Charge,
    vm_id: usize,
    gpa: u64,
    src: &[u8],
) -> ViResult<usize> {
    crate::arch::vm::write_guest_memory(owner, vm_id, gpa, src)
}

/// Copy guest physical RAM at `gpa` into `dst`.
///
/// `dst` is kernel memory (the syscall layer copies it out through the
/// caller's copy view); the whole `[gpa, gpa + len)` range must lie inside the
/// guest-RAM window or nothing is read.
pub(crate) fn read_guest_memory(
    owner: Charge,
    vm_id: usize,
    gpa: u64,
    dst: &mut [u8],
) -> ViResult<usize> {
    crate::arch::vm::read_guest_memory(owner, vm_id, gpa, dst)
}

/// Queue one virtual IRQ request for the next vCPU run. On Pi the monitor
/// asserts HCR_EL2.VI for that run only; its exit path clears VI before returning
/// to the EL1 host. The software GICC owns enabled/pending/active state.
pub(crate) fn inject_irq(
    owner: Charge,
    vm_id: usize,
    vcpu_id: usize,
    intid: u32,
) -> ViResult<usize> {
    crate::arch::vm::inject_irq(owner, vm_id, vcpu_id, intid)
}

// ── Teardown — once per incarnation, at root retirement ──────────────────────

/// Reclaim every VM, its guest RAM and its guest charge, that `owner` holds.
///
/// Called from the root-retirement funnel beside `release_shm_for`, once the
/// incarnation's every member has quiesced, so no vCPU of it can be running.
/// Lock order: VM_REGISTRY → FRAME_ALLOCATOR (same as grant reaper).
pub(crate) fn reap_vms_for(owner: Charge) {
    crate::arch::vm::reap_vms_for(owner)
}
