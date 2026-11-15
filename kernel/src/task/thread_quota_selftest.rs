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

/// The parent incarnation's generation; its quota row is registered with it.
const PARENT_GENERATION: u64 = 1;

fn register_quota(memory: usize) {
    cell_quota::register(
        CellId(QUOTA_CELL),
        PARENT_GENERATION,
        cell_quota::Limits { memory, guest: 0 },
    );
}

/// Bytes one thread occupies: a kernel stack with its guard frames, plus a user
/// stack, whose guards are arena VA with no frame.
fn stack_charge_bytes() -> usize {
    let pages = crate::task::stack_pages_for("thread");
    super::stack::Stack::kernel_reserved_bytes(pages)
        + super::stack::Stack::user_reserved_bytes(pages)
}

fn insert_parent() {
    let mut parent = alloc::boxed::Box::new(Task::new(
        PARENT_TID,
        CellId(QUOTA_CELL),
        "selftest",
        alloc::vec::Vec::new(),
    ));
    parent.cell_generation = PARENT_GENERATION;
    parent.root_tid = PARENT_TID;
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if (QUOTA_CELL as usize) < crate::memory::cell_quota::MAX_CELLS {
            let owner =
                abi::cell_owner::CellOwner::new(QUOTA_CELL, PARENT_GENERATION, PARENT_TID as u64);
            sched.publish_live_cell_owner(owner);
        }
        #[cfg(feature = "p01-trace")]
        crate::memory::heap::p01_owner::task_backing(&parent);
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            None,
            crate::memory::heap::p01_owner::SCHEDULER_SHARED,
        );
        sched.tasks.insert(PARENT_TID, parent);
    }
}

fn remove(tid: usize) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.remove(&tid) {
            if (task.cell_id.0 as usize) < crate::memory::cell_quota::MAX_CELLS {
                let owner = abi::cell_owner::CellOwner::new(
                    task.cell_id.0,
                    task.cell_generation,
                    task.root_tid as u64,
                );
                sched.clear_live_cell_owner_for_test(owner);
            }
        }
    }
    crate::sched::hart_local::ready::remove_from_all(tid);
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

/// Charge appears at spawn and is gone once the dead thread's stacks are freed.
fn charge_then_release(expected: usize) -> bool {
    register_quota(cell_quota::DEFAULT_QUOTA_BYTES);
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
            // Death by the funnel every real death path uses, then the reap that
            // frees the stacks: their runs refund the charge with their frames,
            // and not before.
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                sched.exit_task(tid, 0);
            }
            let held = cell_quota::in_use(CellId(QUOTA_CELL));
            if held != expected {
                ok = false;
                log::error!(
                    "[selftest] THREAD-QUOTA: FAIL — {} bytes charged between exit and reap, expected {}",
                    held,
                    expected
                );
            }
            reap();
            let after = cell_quota::in_use(CellId(QUOTA_CELL));
            if after != 0 {
                ok = false;
                log::error!(
                    "[selftest] THREAD-QUOTA: FAIL — {} bytes still charged after reap",
                    after
                );
            }
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
    register_quota(crate::memory::paging::PAGE_SIZE);
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

/// Thread spawn/exit rounds in [`stack_arena_self_test`].
#[cfg(all(feature = "test-hooks", cellos_wide))]
const ARENA_ROUNDS: usize = 512;

/// How long [`stack_arena_self_test`] lets the frame-run reaper return slots
/// still quarantined behind an unconfirmed flush.
#[cfg(all(feature = "test-hooks", cellos_wide))]
const ARENA_DRAIN_NS: u64 = 2_000_000_000;

/// Returns true iff every thread's user stack maps from the stack arena (never
/// at its physical address) and none of the slots `ARENA_ROUNDS`
/// spawn/exit/reap rounds took (random under randomised slot order) is still
/// held once the frame-run reaper has drained: a slot whose run waited behind
/// an unconfirmed flush returns only when the reaper frees it, and a slot held
/// by a live task's user stack (another hart spawning meanwhile) is not a
/// leak. Slots the test never took are not compared. Logs
/// `[selftest] STACK-ARENA`. Leaves `next_task_id` as found, as [`self_test`].
#[cfg(all(feature = "test-hooks", cellos_wide))]
pub fn stack_arena_self_test() -> bool {
    use crate::loader::va_alloc;

    let saved_next_tid = super::SCHEDULER.lock().as_ref().map(|s| s.next_task_id);
    register_quota(cell_quota::DEFAULT_QUOTA_BYTES);
    insert_parent();

    let mut taken = alloc::collections::BTreeSet::new();
    let mut failure = None;
    for round in 0..ARENA_ROUNDS {
        let tid = match handle_syscall(
            PARENT_TID,
            Syscall::Spawn {
                entry: 0x1000,
                arg: 0,
            },
        ) {
            Ok(tid) if tid != 0 => tid,
            other => {
                failure = Some(alloc::format!("round {round}: spawn returned {other:?}"));
                break;
            }
        };
        let placed = super::SCHEDULER.lock().as_mut().and_then(|sched| {
            let stack = sched.tasks.get(&tid)?.user_stack.as_ref()?;
            let base = stack.base;
            let in_arena = va_alloc::in_stack_arena(base);
            let off_identity = stack.usable_phys() != Some(stack.usable_start());
            sched.exit_task(tid, 0);
            Some((base, in_arena && off_identity))
        });
        reap();
        match placed {
            Some((base, true)) => {
                taken.insert(base);
            }
            _ => {
                failure = Some(alloc::format!(
                    "round {round}: user stack placed {placed:?}"
                ));
                break;
            }
        }
    }

    remove(PARENT_TID);
    cell_quota::deregister(CellId(QUOTA_CELL));
    if let (Some(sched), Some(n)) = (super::SCHEDULER.lock().as_mut(), saved_next_tid) {
        sched.next_task_id = n;
    }

    // A taken slot still held with no live user stack on it, once drained.
    let leaked = || {
        let live: alloc::collections::BTreeSet<usize> = super::SCHEDULER
            .lock()
            .as_ref()
            .map(|sched| {
                sched
                    .tasks
                    .values()
                    .filter_map(|task| task.user_stack.as_ref().map(|stack| stack.base))
                    .collect()
            })
            .unwrap_or_default();
        taken
            .iter()
            .filter(|base| va_alloc::stack_slot_held(**base) && !live.contains(*base))
            .count()
    };
    if failure.is_none() {
        let start = crate::arch::clock::monotonic_ns();
        loop {
            crate::memory::frame_run::reap();
            let still = leaked();
            if still == 0 {
                break;
            }
            match (start, crate::arch::clock::monotonic_ns()) {
                (Some(start), Some(now)) if now.saturating_sub(start) < ARENA_DRAIN_NS => {
                    core::hint::spin_loop()
                }
                _ => {
                    failure = Some(alloc::format!(
                        "{still} of {} slots taken still held after the reaper drained",
                        taken.len()
                    ));
                    break;
                }
            }
        }
    }
    match failure {
        None => {
            log::info!(
                "[selftest] STACK-ARENA: PASS ({ARENA_ROUNDS} thread spawn/exit rounds, no slot leaked)"
            );
            true
        }
        Some(why) => {
            log::error!("[selftest] STACK-ARENA: FAIL — {why}");
            false
        }
    }
}

/// Test hooks (x86_64): `[selftest] SMAP-GRANT-SLICE`. A real thread of a Sas
/// cell (its user stack from the arena, mapped USER in the shared root)
/// allocates a one-page grant and slices it with `size_out` on its own user
/// stack, through the production `handle_syscall` path. The size must land in
/// that slot, read back through the user-copy window; the slot is primed with
/// a sentinel through the same window first, so a skipped write cannot pass.
/// Under SMAP a raw kernel store to the slot is a kernel `#PF` (fatal), so a
/// PASS with `smap=on` proves the write goes through the window. The domain
/// lane requires the marker when its boot line says `smap=on`. Leaves the
/// scheduler, the quota tables and `next_task_id` as found.
#[cfg(all(
    feature = "test-hooks",
    feature = "native-domains",
    cellos_wide,
    target_arch = "x86_64"
))]
pub fn smap_grant_slice_self_test() -> bool {
    let smap = if crate::arch::user_access::smap_active() {
        "on"
    } else {
        "off"
    };
    let saved_next_tid = super::SCHEDULER.lock().as_ref().map(|s| s.next_task_id);
    register_quota(cell_quota::DEFAULT_QUOTA_BYTES);
    insert_parent();

    let outcome = match handle_syscall(
        PARENT_TID,
        Syscall::Spawn {
            entry: 0x1000,
            arg: 0,
        },
    ) {
        Ok(tid) if tid != 0 => {
            let outcome = grant_slice_size_out_lands(tid);
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                sched.exit_task(tid, 0);
            }
            reap();
            outcome
        }
        other => Err(alloc::format!("spawn returned {other:?}")),
    };

    remove(PARENT_TID);
    cell_quota::deregister(CellId(QUOTA_CELL));
    if let (Some(sched), Some(n)) = (super::SCHEDULER.lock().as_mut(), saved_next_tid) {
        sched.next_task_id = n;
    }
    match outcome {
        Ok(()) => {
            log::info!(
                "[selftest] SMAP-GRANT-SLICE: PASS (size_out landed on the user stack, smap={smap})"
            );
            true
        }
        Err(why) => {
            log::error!("[selftest] SMAP-GRANT-SLICE: FAIL (smap={smap}) — {why}");
            false
        }
    }
}

/// One `GrantAlloc` / `GrantSlice(size_out)` / `GrantFree` round by `tid`,
/// with `size_out` at the top word of its user stack.
#[cfg(all(
    feature = "test-hooks",
    feature = "native-domains",
    cellos_wide,
    target_arch = "x86_64"
))]
fn grant_slice_size_out_lands(tid: usize) -> Result<(), alloc::string::String> {
    use alloc::format;
    use core::mem::size_of;
    const SENTINEL: usize = 0x5a5a_5a5a_5a5a_5a5a;
    let size = crate::memory::paging::PAGE_SIZE;

    let (slot, view) = super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| {
            let task = sched.tasks.get(&tid)?;
            let stack = task.user_stack.as_ref()?;
            Some((
                stack.top - size_of::<usize>(),
                super::copy_glue::TaskCopyView::of(task),
            ))
        })
        .ok_or_else(|| format!("thread {tid} has no user stack"))?;
    if !view.is_sas() {
        return Err(format!("thread {tid} is not a Sas task"));
    }
    view.write_bytes(slot, &SENTINEL.to_ne_bytes())
        .map_err(|()| format!("priming slot {slot:#x} through the window failed"))?;
    let grant_id = match handle_syscall(tid, Syscall::GrantAlloc { size }) {
        Ok(id) if id != 0 => id,
        other => return Err(format!("GrantAlloc returned {other:?}")),
    };
    let sliced = handle_syscall(
        tid,
        Syscall::GrantSlice {
            grant_id,
            size_out_ptr: slot,
            expected_owner_tid: 0,
            flags: 0,
        },
    );
    let mut landed = [0u8; size_of::<usize>()];
    let read = view.read_into(slot, &mut landed);
    let freed = handle_syscall(tid, Syscall::GrantFree { grant_id });
    if !matches!(sliced, Ok(base) if base == grant_id) {
        return Err(format!(
            "GrantSlice returned {sliced:?}, expected {grant_id:#x}"
        ));
    }
    read.map_err(|()| format!("reading slot {slot:#x} back through the window failed"))?;
    let value = usize::from_ne_bytes(landed);
    if value != size {
        return Err(format!("size_out holds {value:#x}, expected {size:#x}"));
    }
    freed.map_err(|error| format!("GrantFree returned {error:?}"))?;
    Ok(())
}
