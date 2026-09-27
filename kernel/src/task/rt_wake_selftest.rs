//! Phase-06 witness: a message consume must request preemption for the RT
//! sender's **actual target hart**, and the request must be decided from that
//! hart's running priority.
//!
//! The observable is the preemption decision (`task::smp::preempt_pends_for`), not
//! the interrupt: the fixture holds `SCHEDULER` for its whole body, so the target
//! hart cannot dispatch the synthetic sender before the fixture removes it, and the
//! property is asserted exactly rather than timed.
//!
//! Properties:
//!   1. consuming the exact message wakes the sender onto its target hart's ready
//!      queue and pends exactly one preemption for that hart;
//!   2. a stale delivery token neither wakes the sender nor pends anything;
//!   3. the decision follows the *target* hart's running priority — the waking hart
//!      is deliberately busy with an equal-priority task, which the old
//!      current-hart predicate treated as "no preemption needed".

use super::ipc_wire::IpcWireHeader;
use super::smp::{is_rt_hart_online, HART_RT};
use super::tcb::{Task, TaskState};
use super::{wake_sender_token, SCHEDULER};
use alloc::boxed::Box;
use alloc::vec::Vec;
use api::TaskPriority;
use types::CellId;

// Synthetic tids above the range the boot sequence assigns, removed before return.
const SENDER_TID: usize = 9601;
const RECEIVER_TID: usize = 9602;
const TARGET_OCCUPANT_TID: usize = 9603;
const WAKING_OCCUPANT_TID: usize = 9604;
const SENDER_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 20) as u64;
const RECEIVER_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 21) as u64;
const OCCUPANT_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 22) as u64;
const SENDER_GENERATION: u64 = 7;
const DELIVERY_ID: u64 = 0x5151;

fn fixture_task(tid: usize, cell: u64, priority: TaskPriority) -> Box<Task> {
    let mut task = Box::new(Task::new(tid, CellId(cell), "rt-wake-fixture", Vec::new()));
    task.cell_generation = SENDER_GENERATION;
    task.root_tid = tid;
    task.priority = priority as u8;
    task
}

fn header(delivery_id: u64) -> IpcWireHeader {
    IpcWireHeader {
        sender_tid: SENDER_TID,
        sender_cell_id: SENDER_CELL,
        sender_generation: SENDER_GENERATION,
        delivery_id,
    }
}

pub(crate) fn self_test() -> bool {
    let rt_online = is_rt_hart_online();
    let waking_hart = super::hart_local::current_hart_id();
    let target_hart = if rt_online { HART_RT } else { waking_hart };
    let cross_hart = target_hart != waking_hart;
    let target_was = super::hart_local::ready::current_task_id_for(target_hart);
    let waking_was = super::hart_local::ready::current_task_id_for(waking_hart);

    let mut guard = SCHEDULER.lock();
    let Some(sched) = guard.as_mut() else {
        return false;
    };

    // Occupants pin each hart's running priority: the target hart runs a Normal
    // task, the waking hart a RealTime one.
    sched
        .tasks
        .insert(SENDER_TID, fixture_task(SENDER_TID, SENDER_CELL, TaskPriority::RealTime));
    sched.tasks.insert(
        RECEIVER_TID,
        fixture_task(RECEIVER_TID, RECEIVER_CELL, TaskPriority::Normal),
    );
    sched.tasks.insert(
        TARGET_OCCUPANT_TID,
        fixture_task(TARGET_OCCUPANT_TID, OCCUPANT_CELL, TaskPriority::Normal),
    );
    sched.tasks.insert(
        WAKING_OCCUPANT_TID,
        fixture_task(WAKING_OCCUPANT_TID, OCCUPANT_CELL, TaskPriority::RealTime),
    );
    if let Some(sender) = sched.tasks.get_mut(&SENDER_TID) {
        sender.state = TaskState::Sending {
            target: RECEIVER_TID,
            delivery_id: DELIVERY_ID,
        };
    }
    super::hart_local::ready::set_current_task_id(target_hart, TARGET_OCCUPANT_TID);
    // The waking hart only needs its own occupant when it is a *different* hart: on a
    // single-hart boot the target hart *is* the waking hart, and it must run the
    // Normal occupant for an RT wake to be worth an interrupt.
    if cross_hart {
        super::hart_local::ready::set_current_task_id(waking_hart, WAKING_OCCUPANT_TID);
    }

    // 1. Consuming the exact message.
    super::smp::reset_preempt_pends();
    wake_sender_token(sched, SENDER_TID, RECEIVER_TID, header(DELIVERY_ID));
    let woke = sched
        .tasks
        .get(&SENDER_TID)
        .is_some_and(|task| matches!(task.state, TaskState::Ready));
    let queued_on_target =
        super::hart_local::ready::test_ready_contains_on_hart(target_hart, SENDER_TID);
    let pended = super::smp::preempt_pends_for(target_hart) == 1;

    // 2. A stale delivery token must neither wake the sender nor pend anything.
    if let Some(sender) = sched.tasks.get_mut(&SENDER_TID) {
        sender.state = TaskState::Sending {
            target: RECEIVER_TID,
            delivery_id: DELIVERY_ID,
        };
    }
    super::hart_local::ready::remove_from_all(SENDER_TID);
    super::smp::reset_preempt_pends();
    wake_sender_token(sched, SENDER_TID, RECEIVER_TID, header(DELIVERY_ID + 1));
    let stale_quiet = matches!(
        sched.tasks.get(&SENDER_TID).map(|task| &task.state),
        Some(TaskState::Sending { .. })
    ) && super::smp::preempt_pends_for(target_hart) == 0;

    // 3. The decision follows the target hart's running priority.
    super::smp::reset_preempt_pends();
    sched.pend_preempt_if_needed(TaskPriority::RealTime as u8);
    let target_hart_decides = !cross_hart || super::smp::preempt_pends_for(target_hart) == 1;
    let waking_hart_quiet =
        !cross_hart || super::smp::preempt_pends_for(waking_hart) == 0;

    // Teardown happens before `SCHEDULER` is released, so the target hart can never
    // observe the synthetic sender in its queue.
    for tid in [SENDER_TID, TARGET_OCCUPANT_TID, WAKING_OCCUPANT_TID] {
        super::hart_local::ready::remove_from_all(tid);
    }
    for tid in [SENDER_TID, RECEIVER_TID, TARGET_OCCUPANT_TID, WAKING_OCCUPANT_TID] {
        sched.tasks.remove(&tid);
    }
    super::hart_local::ready::set_current_task_id(target_hart, target_was);
    super::hart_local::ready::set_current_task_id(waking_hart, waking_was);
    super::smp::reset_preempt_pends();

    let ok = woke
        && queued_on_target
        && pended
        && stale_quiet
        && target_hart_decides
        && waking_hart_quiet;
    if ok {
        log::info!(
            "S22-RV64-RT-WAKE: PASS harts={}",
            if rt_online { 2 } else { 1 }
        );
        log::info!(
            "[rt-wake] sender woken onto hart {target_hart} (cross-hart={cross_hart}) with \
             exactly one preempt pend for the target hart"
        );
    } else {
        log::error!(
            "S22-RV64-RT-WAKE: FAIL woke={woke} queued_on_target={queued_on_target} \
             pended={pended} stale_quiet={stale_quiet} \
             target_hart_decides={target_hart_decides} waking_hart_quiet={waking_hart_quiet}"
        );
    }
    ok
}
