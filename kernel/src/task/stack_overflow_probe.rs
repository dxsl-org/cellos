#![cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
//! U-mode proof that a downward stack overflow traps without stopping boot.

use crate::memory::frame::FRAME_ALLOCATOR;
use crate::memory::paging::{self, Flags};
use crate::task::stack::{CellSegments, Stack};
use crate::task::{SCHEDULER, TRAP_FRAME_SIZE};
use types::{CellId, VAddr, ViError};

extern "C" {
    fn __trap_exit();
}

const NAME: &str = "stack_overflow_probe";
const CELL_ID: CellId = CellId(0xFE);
const ENTRY: VAddr = 0x0001_2000;
const STORE_ZERO_BELOW_SP: u32 = 0xFE01_3C23; // sd zero,-8(sp)

/// Spawn a task whose first instruction writes eight bytes below its usable stack.
///
/// The upper guard page must raise a U-mode store-page-fault. Test-hooks QEMU
/// then proves the faulting task is terminated while unrelated cells continue.
pub fn spawn() -> Result<usize, ViError> {
    let pages = super::stack_pages_for(NAME);
    let kstack = Stack::new_kernel(pages)?;
    let ustack = Stack::new_user(pages)?;
    let kernel_stack_top = kstack.top;
    let overflow_sp = ustack.usable_start();
    let overflow_target = overflow_sp.checked_sub(8).ok_or(ViError::NotSupported)?;
    if paging::virt_to_phys(overflow_target).is_some() {
        return Err(ViError::NotSupported);
    }
    let guard_pages = ustack.guard_pages;

    let segments = map_probe_code()?;
    let tid = {
        let mut scheduler_guard = SCHEDULER.lock();
        let scheduler = scheduler_guard.as_mut().ok_or(ViError::Unknown)?;
        scheduler.spawn_with_stacks_configured(
            NAME,
            CELL_ID,
            alloc::vec::Vec::new(),
            kstack,
            ustack,
            move |task| {
                task.segment_mem = Some(segments);
                task.trap_frame.sepc = ENTRY as _;
                task.trap_frame.sstatus = 0x6020_u64 as _;
                task.trap_frame.regs[2] = overflow_sp as _;

                let trap_frame_ptr = kernel_stack_top - TRAP_FRAME_SIZE;
                // SAFETY: the task owns this aligned mapped kernel-stack slot.
                unsafe {
                    *(trap_frame_ptr as *mut crate::hal::arch::ViTrapFrame) = task.trap_frame;
                }
                task.context.sp = trap_frame_ptr as _;
                task.context.ra = __trap_exit as *const () as usize;
                task.context.sstatus = 0x42120;
            },
        )
    };

    log::info!(
        "[stack-guard] deliberate overflow armed guard_pages={} target={:#x}",
        guard_pages,
        overflow_target
    );
    Ok(tid)
}

fn map_probe_code() -> Result<CellSegments, ViError> {
    let mut frame_guard = FRAME_ALLOCATOR.lock();
    let allocator = frame_guard.as_mut().ok_or(ViError::OutOfMemory)?;
    if paging::virt_to_phys(ENTRY).is_some() {
        return Err(ViError::NotSupported);
    }
    let code_frame = allocator.allocate_frame().ok_or(ViError::OutOfMemory)?;

    // SAFETY: the newly allocated identity-mapped frame is exclusively owned.
    unsafe {
        (code_frame as *mut u32).write(STORE_ZERO_BELOW_SP);
    }
    let flags = Flags::from_bits(
        Flags::VALID | Flags::READ | Flags::EXECUTE | Flags::USER | Flags::ACCESSED | Flags::DIRTY,
    );
    if paging::map_page(allocator, ENTRY, code_frame, flags).is_err() {
        allocator.deallocate_frame(code_frame);
        return Err(ViError::OutOfMemory);
    }
    Ok(CellSegments::new(alloc::vec![(ENTRY, code_frame)], 0))
}
