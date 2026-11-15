use super::{copy_glue::TaskCopyView, stack::Stack, syscall::SyscallError, SCHEDULER};

fn usable_stack_bounds(stack: &Stack) -> Result<(usize, usize), SyscallError> {
    let start = stack.usable_start();
    if start > stack.top {
        return Err(SyscallError::InvalidInput);
    }
    Ok((start, stack.top))
}

fn validate_stack_usize_slot(stack: &Stack, ptr: usize) -> Result<*mut usize, SyscallError> {
    if ptr == 0 || !ptr.is_multiple_of(core::mem::align_of::<usize>()) {
        return Err(SyscallError::InvalidInput);
    }
    let end = ptr
        .checked_add(core::mem::size_of::<usize>())
        .ok_or(SyscallError::InvalidInput)?;
    let (start, limit) = usable_stack_bounds(stack)?;
    if ptr < start || end > limit {
        return Err(SyscallError::InvalidInput);
    }
    Ok(ptr as *mut usize)
}

/// A validated optional `usize` output slot on the caller's user stack, with
/// the copy view its write must go through.
pub(super) struct UsizeOutSlot {
    ptr: usize,
    view: TaskCopyView,
}

/// Resolve an optional `usize` output pointer: it must name a whole aligned
/// slot inside the caller's usable user stack, and the caller's copy view must
/// prove it writable now. `0` means "no output". Every refusal happens here,
/// before the caller commits anything, so the later write has no new failure
/// source while the caller stays in this syscall.
pub(super) fn resolve_optional_usize_slot(
    caller_id: usize,
    ptr: usize,
) -> Result<Option<UsizeOutSlot>, SyscallError> {
    if ptr == 0 {
        return Ok(None);
    }
    let view = {
        let guard = SCHEDULER.lock();
        let sched = guard.as_ref().ok_or(SyscallError::PermissionDenied)?;
        let task = sched
            .tasks
            .get(&caller_id)
            .ok_or(SyscallError::PermissionDenied)?;
        let stack = task
            .user_stack
            .as_ref()
            .ok_or(SyscallError::PermissionDenied)?;
        validate_stack_usize_slot(stack, ptr)?;
        TaskCopyView::of(task)
    };
    view.validate_writable(ptr, core::mem::size_of::<usize>())
        .map_err(|()| SyscallError::InvalidInput)?;
    Ok(Some(UsizeOutSlot { ptr, view }))
}

/// Write a slot resolved by [`resolve_optional_usize_slot`].
///
/// The slot is user memory, so the write goes through the caller's copy view
/// (the user-copy window: STAC/CLAC under x86 SMAP), never a raw store. The
/// syscall runs in the caller's own context, so its stack mapping stays live
/// between resolution and this write and the copy cannot fail; if it ever
/// does, the value is dropped and logged, and the caller's result stands.
/// Callers must not hold a grant-table lock here.
pub(super) fn write_resolved_optional_usize(slot: Option<UsizeOutSlot>, value: usize) {
    let Some(slot) = slot else {
        return;
    };
    if slot
        .view
        .write_bytes(slot.ptr, &value.to_ne_bytes())
        .is_err()
    {
        log::warn!(
            "[user_out] usize out-slot {:#x} refused after validation; value not written",
            slot.ptr
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sched::scheduler::Scheduler;
    use crate::sync::SpinlockGuard;
    use crate::task::tcb::Task;
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use types::CellId;

    struct SchedulerTestGuard {
        _guard: SpinlockGuard<'static, ()>,
        saved: Option<Scheduler>,
    }

    impl SchedulerTestGuard {
        fn new() -> Self {
            let guard = crate::TEST_STATE_LOCK.lock();
            let mut scheduler = SCHEDULER.lock();
            let saved = scheduler.take();
            Self {
                _guard: guard,
                saved,
            }
        }

        fn set(&mut self, scheduler: Scheduler) {
            *SCHEDULER.lock() = Some(scheduler);
        }
    }

    impl Drop for SchedulerTestGuard {
        fn drop(&mut self) {
            *SCHEDULER.lock() = self.saved.take();
        }
    }

    fn fake_stack() -> Stack {
        Stack::for_tests(0x8000, 2, 2, 0xc000)
    }

    #[test]
    fn accepts_aligned_slot_inside_usable_stack() {
        let ptr = fake_stack().usable_start();
        assert_eq!(
            validate_stack_usize_slot(&fake_stack(), ptr).unwrap() as usize,
            ptr
        );
    }

    #[test]
    fn rejects_null_misaligned_guard_and_past_top_slots() {
        let stack = fake_stack();
        let usable_start = stack.usable_start();
        assert_eq!(
            validate_stack_usize_slot(&stack, 0).unwrap_err(),
            SyscallError::InvalidInput
        );
        assert_eq!(
            validate_stack_usize_slot(&stack, usable_start + 1).unwrap_err(),
            SyscallError::InvalidInput
        );
        assert_eq!(
            validate_stack_usize_slot(&stack, stack.base).unwrap_err(),
            SyscallError::InvalidInput
        );
        assert_eq!(
            validate_stack_usize_slot(&stack, stack.top - core::mem::size_of::<usize>() + 1)
                .unwrap_err(),
            SyscallError::InvalidInput
        );
    }

    #[test]
    fn rejects_missing_task_and_missing_stack() {
        let mut scheduler = SchedulerTestGuard::new();
        scheduler.set(Scheduler::new());
        assert!(matches!(
            resolve_optional_usize_slot(99, fake_stack().usable_start()),
            Err(SyscallError::PermissionDenied)
        ));

        let mut task = Box::new(Task::new(7, CellId(7), "nostack", Vec::new()));
        task.user_stack = None;
        {
            let mut guard = SCHEDULER.lock();
            let sched = guard.as_mut().unwrap();
            sched.tasks.insert(7, task);
        }
        assert!(matches!(
            resolve_optional_usize_slot(7, fake_stack().usable_start()),
            Err(SyscallError::PermissionDenied)
        ));
    }
}
