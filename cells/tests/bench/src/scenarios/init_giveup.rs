//! Watch `init`'s crash-storm budget engage.
//!
//! `init` restarts a `Permanent` service and gives up on it after five restarts inside one
//! window (`MAX_RESTARTS_PER_WINDOW`, Spec 12 §4.3). The window is a scheduler-tick count; when
//! it was compared against `GetTime` op 0 — the raw architected counter, 10 MHz `mtime` on RV64
//! — it was ~0.1 ms wide, so the budget rolled on every exit and a crash-looping service was
//! restarted forever.
//!
//! This scenario makes the storm real: it force-exits the service cell six times in a row and
//! requires that `init` stops bringing it back. It runs in the orchestrator context, which is
//! the one holding `SpawnCap` (`ForceExit` is gated on that capability).
//!
//! Markers: `[init-giveup] PASS` / `[init-giveup] FAIL — <reason>`.

use alloc::format;
use api::syscall::service;
use ostd::{
    io::println,
    syscall::{
        sys_exit, sys_force_exit, sys_get_scheduler_ticks, sys_lookup_service, SyscallResult,
    },
    task::yield_now,
};

/// The service `init` supervises that this scenario kills. `/bin/config` is `Permanent`, holds
/// no block/network capability (so `ForceExit` is allowed), and registers a well-known id — which
/// is what makes the give-up observable from userspace.
const TARGET_SERVICE: u16 = service::CONFIG;
const TARGET_NAME: &str = "/bin/config";

/// Abnormal exits to deliver. `init` checks `restart_count >= MAX_RESTARTS_PER_WINDOW` *before*
/// incrementing, so five restarts are allowed and the sixth abnormal exit is the one it refuses.
const KILLS: u32 = 6;
/// How long one restart may take before the scenario calls it a failure.
const RESTART_WAIT_TICKS: u64 = 300;
/// How long the service must stay down after the last kill for the give-up to count.
const ABSENT_TICKS: u64 = 200;

fn now() -> u64 {
    sys_get_scheduler_ticks().unwrap_or(0)
}

fn fail(why: &str) -> ! {
    println(&format!("[init-giveup] FAIL — {why}"));
    sys_exit(1);
}

pub fn run() -> ! {
    println(&format!(
        "[init-giveup] START: {KILLS} forced exits on {TARGET_NAME}"
    ));

    for kill in 1..=KILLS {
        let Some(tid) = sys_lookup_service(TARGET_SERVICE) else {
            fail(&format!(
                "kill {kill}: {TARGET_NAME} is already absent — init gave up before its budget was spent"
            ));
        };
        if !matches!(sys_force_exit(tid), SyscallResult::Ok(_)) {
            fail(&format!("kill {kill}: ForceExit refused for tid {tid}"));
        }
        println(&format!("[init-giveup] kill {kill}/{KILLS} tid={tid}"));

        let deadline = now() + RESTART_WAIT_TICKS;
        while now() < deadline {
            match sys_lookup_service(TARGET_SERVICE) {
                Some(new_tid) if new_tid != tid => {
                    println(&format!("[init-giveup] restart {kill}: tid={new_tid}"));
                    break;
                }
                _ => yield_now(),
            }
        }
    }

    // The budget is spent: the service must stay down for a whole window's worth of ticks.
    let deadline = now() + ABSENT_TICKS;
    while now() < deadline {
        if let Some(tid) = sys_lookup_service(TARGET_SERVICE) {
            fail(&format!(
                "init restarted {TARGET_NAME} again (tid={tid}) — the crash-storm budget never engaged"
            ));
        }
        yield_now();
    }

    println(&format!(
        "[init-giveup] PASS (init gave up on {TARGET_NAME} after {KILLS} abnormal exits and left it down)"
    ));
    sys_exit(0);
}
