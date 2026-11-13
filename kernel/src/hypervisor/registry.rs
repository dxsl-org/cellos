//! VM registry — per-owner RAII store for Stage-2 tables, guest RAM, and vCPUs.
//!
//! # Lock order
//! `VM_REGISTRY` → `FRAME_ALLOCATOR` (same order as grant reaper; never reverse).
//!
//! # Non-aarch64
//! All public functions return `Err(ViError::NotSupported)` so the compiler
//! produces a complete match for the hypervisor Syscall arms on every target.

use types::ViResult;

/// Byte images of the records the hypervisor syscalls copy to the cell.
#[path = "vmm_wire.rs"]
pub(crate) mod vmm_wire;

// ── Public API ────────────────────────────────────────────────────────────────

/// Allocate guest RAM + Stage-2 table; return opaque `vm_id`.
pub fn create_vm(owner: usize, guest_pages: usize) -> ViResult<usize> {
    crate::arch::vm::create_vm(owner, guest_pages)
}

/// Create a vCPU in `vm_id` with initial PC `entry_pc`; return `vcpu_id` (1-based).
///
/// Under `test-hooks`, writes a P04 HVC smoke blob (`MOVZ X0,#42; HVC #0; B .`)
/// to the page containing `entry_pc` so the test cell does not need userspace
/// memory access to guest RAM.
pub fn create_vcpu(owner: usize, vm_id: usize, entry_pc: u64) -> ViResult<usize> {
    crate::arch::vm::create_vcpu(owner, vm_id, entry_pc)
}

/// Map guest IPA range in `vm_id`'s Stage-2.
pub fn map_guest_memory(
    owner: usize,
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
pub fn guest_window(owner: usize, vm_id: usize) -> ViResult<(u64, usize)> {
    crate::arch::vm::guest_window(owner, vm_id)
}

/// World-switch into the vCPU and return the exit, converted to the API ABI.
/// The syscall layer copies it to the cell; this never touches cell memory.
pub fn run_vcpu(
    owner: usize,
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
pub fn vcpu_regs(
    owner: usize,
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
pub fn write_guest_memory(owner: usize, vm_id: usize, gpa: u64, src: &[u8]) -> ViResult<usize> {
    crate::arch::vm::write_guest_memory(owner, vm_id, gpa, src)
}

/// Copy guest physical RAM at `gpa` into `dst`.
///
/// `dst` is kernel memory (the syscall layer copies it out through the
/// caller's copy view); the whole `[gpa, gpa + len)` range must lie inside the
/// guest-RAM window or nothing is read.
pub fn read_guest_memory(owner: usize, vm_id: usize, gpa: u64, dst: &mut [u8]) -> ViResult<usize> {
    crate::arch::vm::read_guest_memory(owner, vm_id, gpa, dst)
}

/// Queue one virtual IRQ request for the next vCPU run. On Pi the monitor
/// asserts HCR_EL2.VI for that run only; its exit path clears VI before returning
/// to the EL1 host. The software GICC owns enabled/pending/active state.
pub fn inject_irq(owner: usize, vm_id: usize, vcpu_id: usize, intid: u32) -> ViResult<usize> {
    crate::arch::vm::inject_irq(owner, vm_id, vcpu_id, intid)
}

// ── Teardown — called on every task-exit path ─────────────────────────────────

/// Reclaim all VMs and guest RAM owned by `dead_tid`.
///
/// Called alongside `reap_grants_for_task` on task exit, fault, and watchdog kill.
/// Lock order: VM_REGISTRY → FRAME_ALLOCATOR (same as grant reaper).
pub fn reap_vms_for_task(dead_tid: usize) {
    crate::arch::vm::reap_vms_for_task(dead_tid)
}

/// Pi's EL1-host capability gate: an actual monitor round-trip must run guest
/// HVC, WFI, MMIO and software-injected virtual IRQ before any VM is admitted.
/// Called only when the board has an EL2 monitor (`El2Monitor::PRESENT`).
#[cfg(target_arch = "aarch64")]
pub fn pi_monitor_smoke() {
    crate::arch::vm::pi_monitor_smoke()
}
