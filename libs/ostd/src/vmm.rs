//! VMM syscall wrappers (220-227) for cells holding `HypervisorCap`.
//!
//! One safe function per kernel VMM call, shared by the silo and hypervisor
//! services and the hypervisor ABI smoke test. Every function returns the raw
//! kernel result register: an id, a byte count or 0 on success, and a value
//! the caller compares against its documented error sentinels (0 and/or
//! `usize::MAX`) on failure. The kernel gates every call on `HypervisorCap`
//! and returns an error to a cell without it.
//!
//! Guest-memory invariant behind every call: guest RAM is a run of frames the
//! kernel carves from its own frame allocator at `CreateVm`
//! (`Stage2Table::carve_guest_ram`), and stage 2 maps only that carve. No byte of
//! the calling cell's memory is guest-visible, and the cell never holds a
//! pointer into guest RAM — it moves bytes only through kernel copies that
//! bounds-check the cell side (`validate_user_buf` + the caller copy view) and
//! the guest side (`check_guest_range`). The only cell memory the kernel writes
//! is the out-parameter of the call in progress, which the wrapper borrows
//! exclusively.
#![allow(unsafe_code)]

use abi::hypervisor::ViVmExit;
use abi::syscall::ViSyscall;

use crate::syscall::syscall;

/// Scheduler budget the VMM services pass to each [`run_vcpu`] call (~10 ms).
pub const SCHED_TICK_BUDGET_NS: u64 = 10_000_000;

/// Issue VMM syscall `id`, returning the raw result register unchanged.
///
/// # Safety
/// Same contract as [`crate::syscall::syscall`]: every argument `id` reads as a
/// cell address must be valid for the kernel's access over the length that
/// call uses, and only the caller's exclusive borrow may observe bytes the
/// kernel writes.
#[inline(always)]
unsafe fn vmm_call(id: ViSyscall, a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    // The kernel result register is a machine word; the VMM callers compare
    // its bit pattern against `usize` sentinels, so reinterpret, not convert.
    // SAFETY: forwarded verbatim from this function's `# Safety` contract.
    unsafe { syscall(id, a0, a1, a2, a3) as usize }
}

/// Allocate guest RAM + Stage-2 table; returns vm_id, or 0 / `usize::MAX` on error.
pub fn create_vm(guest_pages: usize) -> usize {
    // SAFETY: register-only. The kernel carves the guest RAM from its own frame
    // allocator; it reads and writes no memory of this cell.
    unsafe { vmm_call(ViSyscall::CreateVm, guest_pages, 0, 0, 0) }
}

/// Create a vCPU with initial PC `entry_pc` in `vm_id`; returns vcpu_id, or 0 /
/// `usize::MAX` on error.
pub fn create_vcpu(vm_id: usize, entry_pc: u64) -> usize {
    // SAFETY: register-only. `entry_pc` is a guest IPA, meaningful only through
    // the VM's stage-2 table; no cell memory is accessed.
    unsafe { vmm_call(ViSyscall::CreateVcpu, vm_id, entry_pc as usize, 0, 0) }
}

/// Map guest IPA range in `vm_id`; returns 0 on success.
pub fn map_guest_memory(vm_id: usize, ipa: u64, size: usize, writable: bool) -> usize {
    // SAFETY: register-only. The kernel rejects `ipa + size` overflow and any
    // `size` larger than the VM's carve, and backs the range with carve frames
    // only, so stage 2 can never expose this cell's memory to the guest.
    unsafe {
        vmm_call(
            ViSyscall::MapGuestMemory,
            vm_id,
            ipa as usize,
            size,
            writable as usize,
        )
    }
}

/// Copy `src` bytes into guest RAM at `gpa`; returns bytes written or an error.
pub fn write_guest_memory(vm_id: usize, gpa: u64, src: &[u8]) -> usize {
    // SAFETY: `src` is a live shared borrow of exactly `src.len()` bytes for the
    // whole call. The kernel only reads it (through the caller copy view, after
    // `validate_user_buf`) and writes nothing in this cell; the guest side is
    // bounded to the carve by `check_guest_range`.
    unsafe {
        vmm_call(
            ViSyscall::WriteGuestMemory,
            vm_id,
            gpa as usize,
            src.as_ptr() as usize,
            src.len(),
        )
    }
}

/// Copy `dst.len()` bytes from guest RAM at `gpa` into `dst`; returns bytes read
/// or an error.
pub fn read_guest_memory(vm_id: usize, gpa: u64, dst: &mut [u8]) -> usize {
    // SAFETY: `dst` is exclusively borrowed for the whole call, so no other
    // reference observes the kernel's writes; the kernel writes at most
    // `dst.len()` bytes through the caller copy view after `validate_user_buf`
    // and reads only carve bytes `check_guest_range` admitted. Every byte value
    // is a valid `u8`.
    unsafe {
        vmm_call(
            ViSyscall::ReadGuestMemory,
            vm_id,
            gpa as usize,
            dst.as_mut_ptr() as usize,
            dst.len(),
        )
    }
}

/// World-switch into `vcpu_id` for at most `budget_ns` (0 = no budget); writes
/// the exit reason to `*exit`. Returns 0 or an error.
pub fn run_vcpu(vm_id: usize, vcpu_id: usize, budget_ns: u64, exit: &mut ViVmExit) -> usize {
    // SAFETY: `exit` is exclusively borrowed for the whole call. The kernel
    // writes exactly `size_of::<ViVmExit>()` bytes (80, pinned by const asserts
    // in `abi::hypervisor` and the kernel's `vmm_wire`): first all zeros — tag 0
    // with zero payload, a valid `MmioRead` — then `encode_exit`'s image, whose
    // tag is a declared discriminant, whose `bool`s are 0/1 and whose padding is
    // zero. The guest runs on stage-2 mappings of its carve only, so the world
    // switch cannot reach this cell's memory.
    unsafe {
        vmm_call(
            ViSyscall::RunVcpu,
            vm_id,
            vcpu_id,
            budget_ns as usize,
            exit as *mut ViVmExit as usize,
        )
    }
}

/// Read (`write == false`, mode 0) or write (`write == true`, mode 1) the vCPU
/// GP registers: 32 × u64 = x0..x30 plus the guest PC.
pub fn vcpu_regs(vm_id: usize, vcpu_id: usize, regs: &mut [u64; 32], write: bool) -> usize {
    // SAFETY: `regs` is exclusively borrowed and is exactly the kernel's
    // `REGS_LEN` (32 × u64 = 256 bytes). Every mode reads that buffer through
    // the caller copy view after `validate_user_buf`; mode 0 also writes it
    // back. Every bit pattern is a valid `u64`.
    unsafe {
        vmm_call(
            ViSyscall::VcpuRegs,
            vm_id,
            vcpu_id,
            regs.as_mut_ptr() as usize,
            write as usize,
        )
    }
}

/// VcpuRegs mode 2: read the guest's saved CNTV_CTL/CNTV_CVAL into words 0 and
/// 1, zeroing the remaining words. Only AArch64 kernels built for the Pi
/// monitor accept mode 2; every other kernel rejects it as invalid input.
#[cfg(target_arch = "aarch64")]
pub fn guest_timer_regs(vm_id: usize, vcpu_id: usize, regs: &mut [u64; 32]) -> usize {
    // SAFETY: as `vcpu_regs` mode 0 — the exclusively borrowed 256-byte `regs`
    // is read and then written back whole through the caller copy view.
    unsafe {
        vmm_call(
            ViSyscall::VcpuRegs,
            vm_id,
            vcpu_id,
            regs.as_mut_ptr() as usize,
            2,
        )
    }
}

/// Raise interrupt `intid` on `vcpu_id` of `vm_id` through the kernel's
/// virtual interrupt controller; returns 0 or an error.
pub fn inject_irq(vm_id: usize, vcpu_id: usize, intid: u32) -> usize {
    // SAFETY: register-only; the kernel bounds `intid` and touches no cell
    // memory.
    unsafe { vmm_call(ViSyscall::InjectIrq, vm_id, vcpu_id, intid as usize, 0) }
}

/// The host's time-stamp counter, which an x86 guest reads unchanged: guest
/// RDTSC does not exit and both VMM backends run with a TSC offset of 0.
#[cfg(target_arch = "x86_64")]
pub fn read_tsc() -> u64 {
    // SAFETY: RDTSC reads a counter register only; it touches no memory and
    // the kernel leaves CR4.TSD clear, so it does not fault at CPL 3.
    unsafe { core::arch::x86_64::_rdtsc() }
}
