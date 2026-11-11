//! Phase 10 fixture: a timeout fires while hart 0 is held in a busy loop.
//!
//! Before Phase 10 only hart 0 ran the timer sweep, so a task whose deadline
//! passed stayed parked for as long as hart 0 could not schedule. The fixture
//! parks a synthetic task in `Sleeping` and holds hart 0 with interrupts
//! masked until the sweep, which can now only run on another hart, readies it
//! or a tick budget runs out.
//!
//! The synthetic task has no stack and must never run. While the fixture
//! holds, hart 0 publishes it as its *selected* task, so every other hart's
//! selection and work stealing leave its ready-queue entry deferred
//! (`ready::owned_by_another_hart`); the fixture removes it before restoring
//! hart 0's selection.
//!
//! x86 advances `system_ticks` on hart 0 alone, so no deadline can pass while
//! hart 0 is held; the fixture reports SKIP there, and on single-hart boots.
//! Marker: `[selftest] OFF-HART0-TIMEOUT: PASS|FAIL|SKIP …`.

#[cfg(not(target_arch = "x86_64"))]
use crate::hal::Arch;
#[cfg(not(target_arch = "x86_64"))]
use crate::sched::hart_local::ready;
#[cfg(not(target_arch = "x86_64"))]
use crate::task::{system_ticks, tcb::Task, tcb::TaskState, SCHEDULER};

/// Ticks from parking to the deadline.
#[cfg(not(target_arch = "x86_64"))]
const DELAY_TICKS: usize = 3;
/// Ticks hart 0 waits, past the deadline, before reporting FAIL.
#[cfg(not(target_arch = "x86_64"))]
const BUDGET_TICKS: usize = 50;
/// A tid above the boot range and a Cell id above every boot cell, used only
/// by this fixture.
#[cfg(not(target_arch = "x86_64"))]
const FIXTURE_TID: usize = 9_701;
#[cfg(not(target_arch = "x86_64"))]
const FIXTURE_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 23) as u64;

pub(crate) fn run_primary() {
    #[cfg(target_arch = "x86_64")]
    log::info!("[selftest] OFF-HART0-TIMEOUT: SKIP x86 ticks advance on hart 0 only");
    #[cfg(not(target_arch = "x86_64"))]
    run();
}

#[cfg(not(target_arch = "x86_64"))]
fn run() {
    let harts = crate::sched::smp::online_hart_count();
    if harts < 2 {
        log::info!("[selftest] OFF-HART0-TIMEOUT: SKIP harts={harts}");
        return;
    }
    let selected_was = ready::selected_task_id_for(0);
    let until = system_ticks() + DELAY_TICKS;
    {
        let mut guard = SCHEDULER.lock();
        let Some(sched) = guard.as_mut() else {
            log::error!("[selftest] OFF-HART0-TIMEOUT: FAIL no scheduler");
            return;
        };
        let mut task = alloc::boxed::Box::new(Task::new(
            FIXTURE_TID,
            types::CellId(FIXTURE_CELL),
            "off-hart0-timeout",
            alloc::vec::Vec::new(),
        ));
        task.root_tid = FIXTURE_TID;
        task.state = TaskState::Sleeping { until };
        // Park it the way a sleeping task leaves the CPU: in the table, off
        // every ready queue, noted by the switch-out bookkeeping, and owned
        // by hart 0 so no other hart can dispatch it once it is readied.
        ready::set_selected_task_id(0, FIXTURE_TID);
        sched.tasks.insert(FIXTURE_TID, task);
        sched
            .tasks
            .note_parked(FIXTURE_TID, &TaskState::Sleeping { until });
    }

    // Hold hart 0: no interrupt, so no tick handler and no sweep here. Taking
    // `SCHEDULER` to read the state answers peers' flush requests on the way.
    let interrupts = crate::hal::ARCH.interrupts_enabled();
    crate::hal::ARCH.disable_interrupts();
    let woke_at = loop {
        let now = system_ticks();
        let sleeping = SCHEDULER
            .lock()
            .as_ref()
            .and_then(|sched| sched.tasks.get(&FIXTURE_TID))
            .is_some_and(|task| matches!(task.state, TaskState::Sleeping { .. }));
        if !sleeping {
            break Some(now);
        }
        if now >= until + BUDGET_TICKS {
            break None;
        }
        core::hint::spin_loop();
    };

    // Remove the task before hart 0 gives up its selection, so it is never
    // dispatchable.
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        ready::remove_from_all(FIXTURE_TID);
        sched.tasks.remove(&FIXTURE_TID);
    }
    ready::set_selected_task_id(0, selected_was);
    if interrupts {
        crate::hal::ARCH.enable_interrupts();
    }
    match woke_at {
        Some(at) => log::info!(
            "[selftest] OFF-HART0-TIMEOUT: PASS harts={harts} deadline={until} woke_at={at}"
        ),
        None => log::error!(
            "[selftest] OFF-HART0-TIMEOUT: FAIL harts={harts} deadline={until} still sleeping"
        ),
    }
}
