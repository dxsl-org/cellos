//! Boot self-test for thread-stack quota accounting.
//!
//! A thread's kernel and user stacks are real memory held for the thread's whole
//! life. If they are not charged to the cell that asked for the thread, a cell
//! can grow its true footprint while every number the kernel reports about it
//! stays still — and the only remaining bound is the thread count, which says
//! nothing about memory.
//!
//! Two directions are proven here, because getting one right and the other wrong is
//! worse than neither: a charge that is never released turns the quota into a slow
//! leak that eventually refuses legitimate work, and a release without a charge
//! makes the whole ledger decorative.
//!
//! Runs in the same single-hart window as the other task self-tests — after
//! `task::init()` and before `smp::start_secondaries()` — so the real thread it
//! spawns cannot be picked up by another hart between creation and teardown. The
//! scheduler and the quota tables are left exactly as they were found.

use super::syscall::{handle_syscall, Syscall, SyscallError};
use super::tcb::Task;
use crate::memory::cell_quota;
use types::CellId;

/// Synthetic tid outside any range the boot sequence has assigned yet.
const PARENT_TID: usize = 9201;

/// Quota slot to exercise. Must be `< MAX_CELLS` for the charge to be tracked at
/// all (higher ids are uncapped by design), and is the last slot precisely because
/// cell ids are handed out from `next_task_id` upward — nothing has reached it at
/// this point in boot, and the test deregisters it before anything can.
const QUOTA_CELL: u64 = (cell_quota::MAX_CELLS - 1) as u64;

/// Bytes one thread occupies: kernel stack plus user stack, each with guards.
fn stack_charge_bytes() -> usize {
    2 * (crate::task::stack_pages_for("thread") + crate::task::stack::STACK_GUARD_PAGES)
        * crate::memory::paging::PAGE_SIZE
}

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
        if (QUOTA_CELL as usize) < crate::memory::cell_quota::MAX_CELLS {
            let owner = api::cell_owner::CellOwner::new(QUOTA_CELL, 1, PARENT_TID as u64);
            sched.publish_live_cell_owner(owner);
        }
        sched.tasks.insert(PARENT_TID, parent);
    }
}

fn remove(tid: usize) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.remove(&tid) {
            if (task.cell_id.0 as usize) < crate::memory::cell_quota::MAX_CELLS {
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

/// Charge appears at spawn and is gone once the thread dies.
fn charge_then_release(expected: usize) -> bool {
    cell_quota::register(CellId(QUOTA_CELL), cell_quota::DEFAULT_QUOTA_BYTES);
    insert_parent();

    let mut ok = true;
    let spawned = handle_syscall(
        PARENT_TID,
        Syscall::Spawn {
            entry: 0x1000,
            arg: 0,
        },
    );

    match spawned {
        Ok(tid) if tid != 0 => {
            let charged = cell_quota::in_use(CellId(QUOTA_CELL));
            if charged != expected {
                ok = false;
                log::error!(
                    "[selftest] THREAD-QUOTA: FAIL — charged {} bytes, expected {}",
                    charged,
                    expected
                );
            }
            // Death by the funnel every real death path uses, not by lifting the
            // task out of the table: the refund is only correct if it happens there.
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                sched.exit_task(tid, 0);
            }
            let after = cell_quota::in_use(CellId(QUOTA_CELL));
            if after != 0 {
                ok = false;
                log::error!(
                    "[selftest] THREAD-QUOTA: FAIL — {} bytes still charged after exit",
                    after
                );
            }
            reap();
        }
        other => {
            ok = false;
            log::error!("[selftest] THREAD-QUOTA: FAIL — spawn returned {:?}", other);
        }
    }

    remove(PARENT_TID);
    cell_quota::deregister(CellId(QUOTA_CELL));
    ok
}

/// A cell whose quota cannot absorb another thread's stacks is refused, and the refusal
/// leaves no charge behind.
fn refused_when_unaffordable() -> bool {
    // One page of quota — below a stack by three orders of magnitude, so the
    // refusal cannot be an artefact of a borderline comparison.
    cell_quota::register(CellId(QUOTA_CELL), crate::memory::paging::PAGE_SIZE);
    insert_parent();

    let mut ok = true;
    let refused = handle_syscall(
        PARENT_TID,
        Syscall::Spawn {
            entry: 0x1000,
            arg: 0,
        },
    );
    if !matches!(refused, Err(SyscallError::TryAgain)) {
        ok = false;
        log::error!(
            "[selftest] THREAD-QUOTA: FAIL — unaffordable spawn returned {:?}, expected TryAgain",
            refused
        );
        if let Ok(stray) = refused {
            remove(stray);
        }
    }
    let leaked = cell_quota::in_use(CellId(QUOTA_CELL));
    if leaked != 0 {
        ok = false;
        log::error!(
            "[selftest] THREAD-QUOTA: FAIL — refused spawn left {} bytes charged",
            leaked
        );
    }

    remove(PARENT_TID);
    cell_quota::deregister(CellId(QUOTA_CELL));
    ok
}

/// Returns true iff a thread stack is charged, released, and enforced as specified.
/// Logs a decisive serial line.
///
/// Transparent to the boot sequence: the successful spawn advances `next_task_id`,
/// so the counter is snapshotted on entry and restored on exit — the first real cell
/// still gets the tid it would have without this test, keeping cell-id assignment
/// stable whether or not the test is compiled in.
pub fn self_test() -> bool {
    let saved_next_tid = super::SCHEDULER.lock().as_ref().map(|s| s.next_task_id);

    let expected = stack_charge_bytes();
    let ok = charge_then_release(expected) & refused_when_unaffordable();

    if let (Some(sched), Some(n)) = (super::SCHEDULER.lock().as_mut(), saved_next_tid) {
        sched.next_task_id = n;
    }

    if ok {
        log::info!("[selftest] THREAD-QUOTA: PASS (charged, released, enforced)");
    } else {
        log::error!("[selftest] THREAD-QUOTA: FAIL");
    }
    ok
}
