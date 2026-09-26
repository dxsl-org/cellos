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
//! One warm-up exit plus one window of quiet anchors `init`'s window before the storm, so the
//! first storm death is the roll and all six land in a single window (see the comment in
//! `run`; without it a host that reaches the shell faster than one window reports a verdict it
//! cannot support).
//!
//! Markers: `[init-giveup] PASS` / `[init-giveup] SKIP` / `[init-giveup] FAIL — <reason>`.

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
/// The window `init` measures its budget in (`RESTART_WINDOW_TICKS`).
///
/// A storm has to fit inside one window: five restarts, then the sixth abnormal exit that
/// exhausts the budget. When the host cannot restart the service that fast — a slow QEMU-TCG
/// runner needs seconds per respawn — the window legitimately rolls and `init` is *right* to
/// keep restarting, so the scenario reports `SKIP` with its measurement instead of a verdict
/// it cannot support.
const WINDOW_TICKS: u64 = 1000;
/// Extra quiet ticks on top of one window before the storm starts.
///
/// `init` anchors its window when it *processes* a death, a few ticks after the
/// exit this scenario delivered — and on a loaded runner the observed
/// kill→process latency reaches 85 ticks. Without the margin the storm's first
/// death can land exactly on the boundary (`window_age=1000`, measured on CI
/// 2026-09-26), where the roll slips to the *second* death and the storm still
/// loses one count. 200 ticks covers that latency with room to spare.
const ANCHOR_MARGIN_TICKS: u64 = 200;

fn now() -> u64 {
    sys_get_scheduler_ticks().unwrap_or(0)
}

fn fail(why: &str) -> ! {
    println(&format!("[init-giveup] FAIL — {why}"));
    sys_exit(1);
}

/// Force one exit, then wait for `init` to bring the service back. Returns the
/// tick at which the exit was delivered.
fn force_exit_and_await_restart(label: &str) -> u64 {
    let Some(tid) = sys_lookup_service(TARGET_SERVICE) else {
        fail(&format!(
            "{label}: {TARGET_NAME} is already absent — init gave up before its budget was spent"
        ));
    };
    if !matches!(sys_force_exit(tid), SyscallResult::Ok(_)) {
        fail(&format!("{label}: ForceExit refused for tid {tid}"));
    }
    let killed_at = now();
    println(&format!("[init-giveup] {label}: tid={tid}"));

    let deadline = killed_at + RESTART_WAIT_TICKS;
    while now() < deadline {
        match sys_lookup_service(TARGET_SERVICE) {
            Some(new_tid) if new_tid != tid => {
                println(&format!(
                    "[init-giveup] {label}: restarted tid={new_tid} after {} ticks",
                    now() - killed_at
                ));
                break;
            }
            _ => yield_now(),
        }
    }
    killed_at
}

pub fn run() -> ! {
    println(&format!(
        "[init-giveup] START: {KILLS} forced exits on {TARGET_NAME}"
    ));

    // Anchor `init`'s window before the storm.
    //
    // The budget is a rate: five restarts per 1000-tick window, and the window
    // rolls at the first death *after* it expires. A service that has not died
    // since boot still carries `window_start = 0`, so a first death younger than
    // one window rolls nothing — and then the roll lands *inside* the storm, one
    // restart is charged to the previous window, the count never reaches the
    // limit, and this witness reports a FAIL its host cannot support. Measured
    // on CI (2026-09-26): boot + command in under 1000 ticks, six restarts in
    // ~150 ticks, no give-up; locally the same storm runs with the first death
    // at tick 1642 and the give-up lands exactly at the sixth.
    //
    // One warm-up exit plus one full window of quiet (plus a margin for the
    // kill→process latency `init` adds on top of our own clock) makes the anchor
    // certain: `window_start <= warmup_death < storm_start - WINDOW_TICKS`, so
    // the storm's first death is guaranteed to be the roll and all six exits fit
    // one window. A death *inside* the quiet window moves the anchor, so the
    // window restarts from the last absence observed — the observed time is never
    // earlier than init's own anchor, so the guarantee survives.
    let quiet_ticks = WINDOW_TICKS + ANCHOR_MARGIN_TICKS;
    let mut window_start = force_exit_and_await_restart("warmup");
    while now().wrapping_sub(window_start) <= quiet_ticks {
        if sys_lookup_service(TARGET_SERVICE).is_none() {
            window_start = now();
        }
        yield_now();
    }

    let mut first_exit_tick = 0u64;
    let mut last_exit_tick = 0u64;
    for kill in 1..=KILLS {
        let killed_at = force_exit_and_await_restart(&format!("kill {kill}/{KILLS}"));
        if kill == 1 {
            first_exit_tick = killed_at;
        }
        last_exit_tick = killed_at;
    }

    // If the six exits did not fit inside one window, `init` was right to keep restarting and
    // this host simply cannot host the experiment. Say so with the measurement rather than
    // reporting a verdict the run cannot support.
    let span = last_exit_tick.saturating_sub(first_exit_tick);
    if span > WINDOW_TICKS {
        println(&format!(
            "[init-giveup] SKIP — {KILLS} exits spanned {span} ticks, beyond init's {WINDOW_TICKS}-tick window; this host still restarted {TARGET_NAME} correctly"
        ));
        sys_exit(0);
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
