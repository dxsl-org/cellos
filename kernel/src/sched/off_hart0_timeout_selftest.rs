//! Phase 10 fixture: a timeout fires while hart 0 is held in a busy loop.
//!
//! Before Phase 10 only hart 0 ran the timer sweep, so a task whose deadline
//! passed stayed parked for as long as hart 0 could not schedule. The fixture
//! parks a real kernel task (a `spawn`ed stack pair running the default task
//! entry) in `Sleeping`, then holds hart 0 with interrupts masked and without
//! `SCHEDULER` until that task leaves `Sleeping` or a tick budget runs out.
//! The sweep can then only run on another hart.
//!
//! x86 advances `system_ticks` on hart 0 alone, so no deadline can pass while
//! hart 0 is held; the fixture reports SKIP there, and on single-hart boots.
//! Marker: `[selftest] OFF-HART0-TIMEOUT: PASS|FAIL|SKIP …`.

#[cfg(not(target_arch = "x86_64"))]
use crate::hal::Arch;
#[cfg(not(target_arch = "x86_64"))]
use crate::task::{system_ticks, tcb::TaskState, SCHEDULER};

/// Ticks from parking to the deadline.
#[cfg(not(target_arch = "x86_64"))]
const DELAY_TICKS: usize = 3;
/// Ticks hart 0 waits, past the deadline, before reporting FAIL.
#[cfg(not(target_arch = "x86_64"))]
const BUDGET_TICKS: usize = 50;
/// A Cell id above every boot cell, used only by this fixture.
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
    let until = system_ticks() + DELAY_TICKS;
    let tid = {
        let mut guard = SCHEDULER.lock();
        let Some(sched) = guard.as_mut() else {
            log::error!("[selftest] OFF-HART0-TIMEOUT: FAIL no scheduler");
            return;
        };
        let tid = match sched.spawn(
            "off-hart0-timeout",
            types::CellId(FIXTURE_CELL),
            alloc::vec::Vec::new(),
        ) {
            Ok(tid) => tid,
            Err(error) => {
                log::error!("[selftest] OFF-HART0-TIMEOUT: FAIL spawn {error:?}");
                return;
            }
        };
        // Park it the way a sleeping task leaves the CPU: off every ready
        // queue, `Sleeping`, and noted by the switch-out bookkeeping.
        crate::sched::hart_local::ready::remove_from_all(tid);
        let state = TaskState::Sleeping { until };
        if let Some(task) = sched.tasks.get_mut(&tid) {
            task.state = state;
        }
        sched.tasks.note_parked(tid, &state);
        tid
    };

    // Hold hart 0: no interrupt, so no tick handler and no sweep here. Taking
    // `SCHEDULER` to read the state answers peers' flush requests on the way.
    let interrupts = crate::hal::ARCH.interrupts_enabled();
    crate::hal::ARCH.disable_interrupts();
    let woke_at = loop {
        let now = system_ticks();
        let sleeping = SCHEDULER
            .lock()
            .as_ref()
            .and_then(|sched| sched.tasks.get(&tid))
            .is_some_and(|task| matches!(task.state, TaskState::Sleeping { .. }));
        if !sleeping {
            break Some(now);
        }
        if now >= until + BUDGET_TICKS {
            break None;
        }
        core::hint::spin_loop();
    };
    if interrupts {
        crate::hal::ARCH.enable_interrupts();
    }

    if let Some(sched) = SCHEDULER.lock().as_mut() {
        sched.exit_task(tid, 0);
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
