//! Boot guard for the "a task id is never re-issued within one boot" invariant.
//!
//! TIDs are handed out from a single monotonic counter, so a retired task's number
//! is never given to a later task. That invariant is load-bearing and is relied on
//! far away from the allocator:
//!
//! * local IPC safety — a cached endpoint that still names a dead tid must fail
//!   closed (`TargetGone`), never reach a *different* provider that happened to
//!   receive the same number;
//! * the service registry records `(tid, cell_id, generation)` per provider and
//!   treats the tid as naming exactly one task for the life of the boot;
//! * the exact-operation IPC path binds a peer as `(tid, cell_id, generation)`.
//!
//! The counter is the only thing enforcing it, so a later change that turns it into
//! a free list — reusing a just-retired number — would silently weaken all three.
//! This guard spawns real threads through the ordinary syscall path, retires each
//! one through the real death funnel, and asserts that no later allocation returns
//! a number that was already issued and that the sequence is strictly increasing.
//!
//! Transparent to the boot sequence: the counter is snapshotted on entry and
//! restored on exit, so the first real cell still receives the tid it would have
//! without this test. Runs in the same single-hart window as the other task
//! self-tests — after `task::init()` and before `smp::start_secondaries()`.

use super::syscall::{handle_syscall, Syscall};
use super::tcb::Task;
use crate::memory::cell_quota;
use types::CellId;

/// Synthetic tid outside any range the boot sequence has assigned yet.
const PARENT_TID: usize = 9211;

/// Quota slot for the spawned threads' stacks. Distinct from the slot the
/// thread-quota self-test uses, so the two cannot observe each other's charges.
const QUOTA_CELL: u64 = (cell_quota::MAX_CELLS - 2) as u64;

/// Spawn/retire cycles to compare. More than one cycle is the whole point: the
/// second allocation is what would collide with a reused number.
const SPAWN_ROUNDS: usize = 3;

fn insert_parent() {
    let mut parent = alloc::boxed::Box::new(Task::new(
        PARENT_TID,
        CellId(QUOTA_CELL),
        "selftest",
        alloc::vec::Vec::new(),
    ));
    parent.cell_generation = 1;
    parent.root_tid = PARENT_TID;
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if (QUOTA_CELL as usize) < cell_quota::MAX_CELLS {
            let owner = api::cell_owner::CellOwner::new(QUOTA_CELL, 1, PARENT_TID as u64);
            sched.publish_live_cell_owner(owner);
        }
        sched.tasks.insert(PARENT_TID, parent);
    }
}

fn remove(tid: usize) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.remove(&tid) {
            if (task.cell_id.0 as usize) < cell_quota::MAX_CELLS {
                let owner = api::cell_owner::CellOwner::new(
                    task.cell_id.0,
                    task.cell_generation,
                    task.root_tid as u64,
                );
                sched.clear_live_cell_owner_for_test(owner);
            }
        }
    }
    super::hart_local::ready::remove_from_all(tid);
}

/// Drop every reapable zombie OUTSIDE the scheduler lock, which is what actually
/// returns a dead thread's stack frames to the allocator.
fn reap() {
    let dead = super::SCHEDULER
        .lock()
        .as_mut()
        .map(|s| s.take_reapable_zombies())
        .unwrap_or_default();
    drop(dead);
}

/// Spawn, retire, spawn again: no issued id may come back, and the sequence must
/// only move forward.
fn ids_are_never_reissued() -> bool {
    cell_quota::register(CellId(QUOTA_CELL), cell_quota::DEFAULT_QUOTA_BYTES);
    insert_parent();

    let mut ok = true;
    let mut issued: alloc::vec::Vec<usize> = alloc::vec::Vec::new();

    for round in 0..SPAWN_ROUNDS {
        match handle_syscall(
            PARENT_TID,
            Syscall::Spawn {
                entry: 0x1000,
                arg: 0,
            },
        ) {
            Ok(tid) if tid != 0 => {
                if issued.contains(&tid) {
                    log::error!(
                        "[selftest] TASK-ID-REUSE: FAIL — id {} was issued again in round {} (issued: {:?})",
                        tid,
                        round,
                        issued
                    );
                    ok = false;
                    break;
                }
                issued.push(tid);
                // Retire through the funnel every real death uses, so the number is
                // genuinely back in the "already issued" set before the next
                // allocation asks for one.
                if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                    sched.exit_task(tid, 0);
                }
                reap();
            }
            other => {
                log::error!(
                    "[selftest] TASK-ID-REUSE: FAIL — spawn in round {} returned {:?}",
                    round,
                    other
                );
                ok = false;
                break;
            }
        }
    }

    if ok && !issued.windows(2).all(|pair| pair[0] < pair[1]) {
        log::error!(
            "[selftest] TASK-ID-REUSE: FAIL — ids were not strictly increasing: {:?}",
            issued
        );
        ok = false;
    }

    remove(PARENT_TID);
    cell_quota::deregister(CellId(QUOTA_CELL));
    ok
}

pub fn self_test() -> bool {
    let saved_next_tid = super::SCHEDULER.lock().as_ref().map(|s| s.next_task_id);

    let ok = ids_are_never_reissued();

    if let (Some(sched), Some(n)) = (super::SCHEDULER.lock().as_mut(), saved_next_tid) {
        sched.next_task_id = n;
    }

    if ok {
        log::info!("[selftest] TASK-ID-REUSE: PASS (monotonic across spawn/exit)");
    }
    ok
}
