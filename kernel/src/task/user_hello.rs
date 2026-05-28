//! Minimal Ring-3 smoke-test task.
//!
//! Allocates a user code page at [`ENTRY`], writes machine code that calls
//! `ecall Log("Hi from U-mode!\n")` then `ecall Exit(0)`, and spawns the task.
//! Used to verify the full U-mode entry path end-to-end.

use crate::memory::frame::FRAME_ALLOCATOR;
use crate::task::stack::Stack;
use crate::task::{SCHEDULER, STACK_PAGES, TRAP_FRAME_SIZE};
use types::{CellId, VAddr, ViError};

extern "C" {
    fn __trap_exit();
}

/// Virtual address where the user hello task's code page is mapped.
///
/// Chosen to be page-aligned and below kernel RAM (0x8000_0000) so it doesn't
/// overlap the identity-mapped kernel or heap-allocator frames. On QEMU virt this
/// region is not occupied by any firmware or device — it sits between the debug ROM
/// (≤ 0x0FFF) and CLINT (0x0200_0000). Verify on non-QEMU targets before reuse.
pub const ENTRY: VAddr = 0x0001_0000;

/// Spawn the Ring-3 hello smoke-test task.
///
/// The task runs entirely in U-mode:
///   1. `ecall` with a7=11  (ViSyscall::Log), a0=msg_ptr, a1=16
///   2. `ecall` with a7=60  (ViSyscall::Exit), a0=0
///
/// # Errors
/// Returns [`ViError::OutOfMemory`] if a frame or stack allocation fails.
/// Returns [`ViError::Unknown`] if the scheduler is uninitialised.
pub fn spawn() -> Result<usize, ViError> {
    // -- 1. Spawn a bare task (allocates TCB, not yet runnable) --
    let tid = super::spawn("user_hello", CellId(0xFF), alloc::vec::Vec::new());
    if tid == 0 {
        return Err(ViError::Unknown);
    }

    // -- 2. Allocate + initialise user code page --
    {
        let mut frame_guard = FRAME_ALLOCATOR.lock();
        let allocator = frame_guard.as_mut().ok_or(ViError::OutOfMemory)?;

        let code_frame = allocator.allocate_frame().ok_or(ViError::OutOfMemory)?;

        // SAFETY: code_frame is a freshly allocated 4 KB frame; identity-mapped
        // (PAddr == VAddr before userspace runs), so the pointer is valid.
        unsafe {
            let base = code_frame as *mut u32;

            // Instruction sequence (8 × 4-byte RISC-V instructions):
            //   lui  a0, 0x10        → a0 = 0x0001_0000 = ENTRY
            *base.add(0) = 0x0001_0537_u32;
            //   addi a0, a0, 32      → a0 = ENTRY + 32  (msg address)
            *base.add(1) = 0x0205_0513_u32;
            //   li   a1, 16          → a1 = 16           (msg length)
            *base.add(2) = 0x0100_0593_u32;
            //   li   a7, 11          → a7 = ViSyscall::Log
            *base.add(3) = 0x00B0_0893_u32;
            //   ecall                → Log(msg, 16)
            *base.add(4) = 0x0000_0073_u32;
            //   li   a0, 0           → exit code = 0
            *base.add(5) = 0x0000_0513_u32;
            //   li   a7, 60          → a7 = ViSyscall::Exit
            *base.add(6) = 0x03C0_0893_u32;
            //   ecall                → Exit(0)
            *base.add(7) = 0x0000_0073_u32;

            // Message at byte offset 32 (= ENTRY + 32 = the a0 address above).
            let msg = b"Hi from U-mode!\n";
            let dst = (code_frame + 32) as *mut u8;
            core::ptr::copy_nonoverlapping(msg.as_ptr(), dst, msg.len());
        }

        // Map the code frame at ENTRY with V|R|X|U — no W, code is read-execute only.
        use crate::memory::paging::Flags;
        let code_flags = Flags::from_bits(
            Flags::VALID
                | Flags::READ
                | Flags::EXECUTE
                | Flags::USER
                | Flags::ACCESSED
                | Flags::DIRTY,
        );
        crate::memory::paging::map_page(allocator, ENTRY, code_frame, code_flags)
            .map_err(|_| ViError::OutOfMemory)?;
    }

    // -- 3. Configure the task to enter U-mode at ENTRY via sret --
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&tid) {
            // sepc = ENTRY; sret will jump here in U-mode.
            task.trap_frame.sepc = ENTRY;
            // sstatus: SPP=0 (U-mode), SPIE=1, FS=Initial (bits 13-14 = 01).
            // Matches spawn_from_mem convention so FP state is consistent.
            task.trap_frame.sstatus = 0x6020;

            let kstack = Stack::new_kernel(STACK_PAGES)
                .map_err(|_| ViError::OutOfMemory)?;
            let ustack = Stack::new_user(STACK_PAGES)
                .map_err(|_| ViError::OutOfMemory)?;

            let kstack_top = kstack.top;
            task.trap_frame.regs[2] = ustack.top; // sp = user stack top
            task.kernel_stack = Some(kstack);
            task.user_stack = Some(ustack);

            // Copy trap frame onto kernel stack; context.sp points to it.
            let tf_ptr = kstack_top - TRAP_FRAME_SIZE;
            // SAFETY: tf_ptr is within the allocated kernel stack and properly aligned.
            unsafe {
                let tf_dst = &mut *(tf_ptr as *mut crate::hal::arch::ViTrapFrame);
                *tf_dst = task.trap_frame;
            }

            // First context switch "returns" to __trap_exit, which reads the
            // trap frame from sp and executes sret into U-mode at sepc=ENTRY.
            task.context.sp = tf_ptr;
            task.context.ra = __trap_exit as *const () as usize;
            // Kernel context sstatus: SUM=1, FS=Initial, SPP=1, SPIE=1.
            // Matches spawn_from_mem convention (0x42120 = SUM | FS=Initial | SPP | SPIE).
            task.context.sstatus = 0x42120;
        }
    }

    Ok(tid)
}
