use super::{stack::Stack, syscall::SyscallError, SCHEDULER};

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

fn resolve_current_task_usize_slot(
    caller_id: usize,
    ptr: usize,
) -> Result<*mut usize, SyscallError> {
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
    validate_stack_usize_slot(stack, ptr)
}

/// Write an optional `usize` out-param into the caller's own user stack.
///
/// Null is accepted. Non-null pointers must be `usize`-aligned and the entire
/// slot must lie within the caller's usable user stack; segment memory and
/// arbitrary user mappings are intentionally rejected.
pub(super) fn write_optional_usize(
    caller_id: usize,
    ptr: usize,
    value: usize,
) -> Result<(), SyscallError> {
    if ptr == 0 {
        return Ok(());
    }
    let slot = resolve_current_task_usize_slot(caller_id, ptr)?;
    // SAFETY: `slot` was validated as a whole aligned `usize` in the current
    // caller's usable user stack while holding SCHEDULER. The lock is dropped
    // before the write, and the syscall runs in the caller's own context.
    unsafe { slot.write(value) };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{SpinLockGuard, Spinlock};
    use crate::task::{scheduler::Scheduler, tcb::Task};
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use types::CellId;

    static TEST_LOCK: Spinlock<()> = Spinlock::new(());

    struct SchedulerTestGuard {
        _guard: SpinLockGuard<'static, ()>,
        saved: Option<Scheduler>,
    }

    impl SchedulerTestGuard {
        fn new() -> Self {
            let guard = TEST_LOCK.lock();
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
        Stack {
            base: 0x8000,
            pages: 2,
            guard_pages: 2,
            top: 0xc000,
        }
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
        assert_eq!(
            resolve_current_task_usize_slot(99, fake_stack().usable_start()).unwrap_err(),
            SyscallError::PermissionDenied
        );

        let mut task = Box::new(Task::new(7, CellId(7), "nostack", Vec::new()));
        task.user_stack = None;
        {
            let mut guard = SCHEDULER.lock();
            let sched = guard.as_mut().unwrap();
            sched.tasks.insert(7, task);
        }
        assert_eq!(
            resolve_current_task_usize_slot(7, fake_stack().usable_start()).unwrap_err(),
            SyscallError::PermissionDenied
        );
    }
}
