use super::super::super::{
    stack::Stack,
    syscall,
    tcb::{Task, TaskState},
    SCHEDULER,
};
use alloc::boxed::Box;
use api::cell_owner::CellOwner;
use types::CellId;

const RAW_GRANT_REGISTER: usize = 215;
const RETIRE_CALLER: usize = 91_305;
const RETIRE_CELL: u64 = 63;

pub(super) const ROOT_RETIRE: usize = 1;
pub(super) const REVOKE: usize = 2;
pub(super) const UNMAP_REUSE: usize = 3;

/// Create a disposable root for the destructive retirement race.
///
/// Returns its caller ID and owned grant base, or `None` when fixture setup fails.
pub(super) fn prepare_root() -> Option<(usize, usize)> {
    let mut root = Box::new(Task::new(
        RETIRE_CALLER,
        CellId(RETIRE_CELL),
        "getrandom-retiring-root",
        alloc::vec::Vec::new(),
    ));
    root.user_stack = Some(Stack::new_user(1).ok()?);
    let owner = CellOwner::new(RETIRE_CELL, root.cell_generation, RETIRE_CALLER as u64);
    {
        let mut guard = SCHEDULER.lock();
        let scheduler = guard.as_mut()?;
        if scheduler.tasks.contains_key(&RETIRE_CALLER)
            || !scheduler.cell_owner_slot_is_empty(CellId(RETIRE_CELL))
            || !scheduler.publish_live_cell_owner(owner)
        {
            return None;
        }
        scheduler.tasks.insert(RETIRE_CALLER, root);
    }
    let grant_base =
        syscall::dispatch_raw_for_test(RETIRE_CALLER, RAW_GRANT_REGISTER, 128, 0, 0, 0)
            .unwrap_or(0);
    if grant_base != 0 {
        Some((RETIRE_CALLER, grant_base))
    } else {
        let _ = retire_root(RETIRE_CALLER);
        None
    }
}

/// Execute one lifecycle action after final authorization releases.
///
/// Returns `(completed, replacement_base)`; only unmap/reuse yields a replacement.
pub(super) fn complete(caller_id: usize, grant_base: usize, mode: usize) -> (bool, usize) {
    match mode {
        ROOT_RETIRE => (retire_root(caller_id), 0),
        REVOKE => (
            syscall::test_unregister_registered_grant_for_race(caller_id, grant_base).is_ok(),
            0,
        ),
        UNMAP_REUSE => {
            if syscall::test_unregister_registered_grant_for_race(caller_id, grant_base).is_err() {
                return (false, 0);
            }
            let reissued =
                syscall::test_reregister_registered_grant_for_race(caller_id, grant_base);
            (reissued, if reissued { grant_base } else { 0 })
        }
        _ => (false, 0),
    }
}

/// Report whether a root entered retirement or completed its asynchronous reap.
pub(super) fn root_is_terminal(caller_id: usize) -> bool {
    SCHEDULER
        .lock()
        .as_ref()
        .is_some_and(|scheduler| match scheduler.tasks.get(&caller_id) {
            Some(task) => matches!(task.state, TaskState::Retiring | TaskState::Terminated),
            None => true,
        })
}

fn retire_root(caller_id: usize) -> bool {
    let mut guard = SCHEDULER.lock();
    let Some(scheduler) = guard.as_mut() else {
        return false;
    };
    scheduler.exit_task(caller_id, 0);
    scheduler
        .tasks
        .get(&caller_id)
        .is_some_and(|task| task.state == TaskState::Retiring)
}
