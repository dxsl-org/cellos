//! Low-level VMM syscall wrappers (220-227) for the silo service cell.
//!
//! ARM64-only at runtime — guarded by the HypervisorCap manifest bit.
//! Other targets compile a stub that returns the `usize::MAX` error sentinel.
//!
//! `unsafe` is required here for the `svc` instruction (hardware syscall ABI)
//! and nowhere else in the crate: the binary root denies `unsafe_code` and only
//! this module re-allows it; the library root forbids it outright.
//!
//! Guest-memory invariant behind every call: guest RAM is a run of frames the
//! kernel carves from its own frame allocator at `CreateVm`
//! (`Stage2Table::carve_guest_ram`), and stage 2 maps only that carve. No byte of
//! this cell's memory is guest-visible, and this cell never holds a pointer into
//! guest RAM — it moves bytes only through kernel copies that bounds-check the
//! cell side (`validate_user_buf` + the caller copy view) and the guest side
//! (`check_guest_range`). The only cell memory the kernel writes is the
//! out-parameter of the call in progress, which the wrapper borrows exclusively.
#![allow(unsafe_code)]

use api::hypervisor::ViVmExit;
use api::syscall::ViSyscall;

/// Scheduler tick budget for each RunVcpu call (~10ms in 10 MHz ticks = 100_000 ticks).
pub const SCHED_TICK_BUDGET_NS: u64 = 10_000_000; // 10ms in nanoseconds

/// Error sentinel returned by VMM syscalls on failure.
#[cfg(not(target_arch = "aarch64"))]
const ERR: usize = usize::MAX;

/// Issue VMM syscall `id` with four register arguments.
///
/// # Safety
/// Every argument `id` interprets as a cell address must name memory that is
/// valid for the kernel's access (read for a source, write for a destination)
/// over the length that syscall uses, and no live Rust reference other than the
/// caller's own exclusive borrow may observe bytes the kernel writes, for the
/// whole call. Register-only arguments carry no obligation.
#[inline]
unsafe fn syscall4(id: ViSyscall, a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    #[allow(unused_mut)] // mut needed for aarch64 inlateout; not needed on other targets
    let mut ret: usize;
    // SAFETY: the CellOS AArch64 SVC ABI (same contract as `ostd::syscall`):
    // x0 = number in / result out, x1-x4 = arguments, every other register and
    // NZCV preserved by the kernel, no user stack touched — exactly the operand
    // list and `nostack, preserves_flags` below. Memory effects are the
    // caller's obligation per `# Safety`.
    #[cfg(target_arch = "aarch64")]
    core::arch::asm!(
        "svc #0",
        inlateout("x0") id as usize => ret,
        in("x1") a0, in("x2") a1, in("x3") a2, in("x4") a3,
        options(nostack, preserves_flags),
    );
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (id, a0, a1, a2, a3);
        ret = ERR;
    }
    ret
}

/// Allocate guest RAM + Stage-2 table; returns vm_id, or 0 / `usize::MAX` on error.
pub fn create_vm(guest_pages: usize) -> usize {
    // SAFETY: register-only. The kernel carves the guest RAM from its own frame
    // allocator; it reads and writes no memory of this cell.
    unsafe { syscall4(ViSyscall::CreateVm, guest_pages, 0, 0, 0) }
}

/// Create a vCPU with initial PC `entry_pc` in `vm_id`; returns vcpu_id, or 0 /
/// `usize::MAX` on error.
pub fn create_vcpu(vm_id: usize, entry_pc: u64) -> usize {
    // SAFETY: register-only. `entry_pc` is a guest IPA, meaningful only through
    // the VM's stage-2 table; no cell memory is accessed.
    unsafe { syscall4(ViSyscall::CreateVcpu, vm_id, entry_pc as usize, 0, 0) }
}

/// Map guest IPA range in `vm_id`; returns 0 on success.
pub fn map_guest_memory(vm_id: usize, ipa: u64, size: usize, writable: bool) -> usize {
    // SAFETY: register-only. The kernel rejects `ipa + size` overflow and any
    // `size` larger than the VM's carve, and backs the range with carve frames
    // only, so stage 2 can never expose this cell's memory to the guest.
    unsafe {
        syscall4(
            ViSyscall::MapGuestMemory,
            vm_id,
            ipa as usize,
            size,
            writable as usize,
        )
    }
}

/// Copy `src` bytes into guest RAM at `gpa`; returns bytes written or ERR.
pub fn write_guest_memory(vm_id: usize, gpa: u64, src: &[u8]) -> usize {
    // SAFETY: `src` is a live shared borrow of exactly `src.len()` bytes for the
    // whole call. The kernel only reads it (through the caller copy view, after
    // `validate_user_buf`) and writes nothing in this cell; the guest side is
    // bounded to the carve by `check_guest_range`.
    unsafe {
        syscall4(
            ViSyscall::WriteGuestMemory,
            vm_id,
            gpa as usize,
            src.as_ptr() as usize,
            src.len(),
        )
    }
}

/// Copy `len` bytes from guest RAM at `gpa` into `dst`; returns bytes read or ERR.
pub fn read_guest_memory(vm_id: usize, gpa: u64, dst: &mut [u8]) -> usize {
    // SAFETY: `dst` is exclusively borrowed for the whole call, so no other
    // reference observes the kernel's writes; the kernel writes at most
    // `dst.len()` bytes through the caller copy view after `validate_user_buf`
    // and reads only carve bytes `check_guest_range` admitted. Every byte value
    // is a valid `u8`.
    unsafe {
        syscall4(
            ViSyscall::ReadGuestMemory,
            vm_id,
            gpa as usize,
            dst.as_mut_ptr() as usize,
            dst.len(),
        )
    }
}

/// World-switch into `vcpu_id`; writes exit reason to `*exit`. Returns 0 or ERR.
pub fn run_vcpu(vm_id: usize, vcpu_id: usize, exit: &mut ViVmExit) -> usize {
    // SAFETY: `exit` is exclusively borrowed for the whole call. The kernel
    // writes exactly `size_of::<ViVmExit>()` bytes (80, pinned by const asserts
    // in `api::hypervisor` and the kernel's `vmm_wire`): first all zeros — tag 0
    // with zero payload, a valid `MmioRead` — then `encode_exit`'s image, whose
    // tag is a declared discriminant, whose `bool`s are 0/1 and whose padding is
    // zero. The guest runs on stage-2 mappings of its carve only, so the world
    // switch cannot reach this cell's memory.
    unsafe {
        syscall4(
            ViSyscall::RunVcpu,
            vm_id,
            vcpu_id,
            SCHED_TICK_BUDGET_NS as usize,
            exit as *mut ViVmExit as usize,
        )
    }
}

/// Read vCPU x0-x30 plus the current guest PC into `regs`.
pub fn read_vcpu_regs(vm_id: usize, vcpu_id: usize, regs: &mut [u64; 32]) -> usize {
    // SAFETY: `regs` is exclusively borrowed and is exactly the kernel's
    // `REGS_LEN` (32 × u64 = 256 bytes). Mode 0 reads that buffer and writes it
    // back through the caller copy view; every bit pattern is a valid `u64`.
    unsafe {
        syscall4(
            ViSyscall::VcpuRegs,
            vm_id,
            vcpu_id,
            regs.as_mut_ptr() as usize,
            0,
        )
    }
}
