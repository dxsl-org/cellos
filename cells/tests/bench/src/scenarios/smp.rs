//! SMP throughput scenarios — validate 2-hart work-stealing on QEMU.
//!
//! All spawn/notify/force_exit helpers run only in the orchestrator context
//! (which has SpawnCap).  Only `run_worker` is called from bench-probe.

use crate::framework::timer::NS_PER_TICK;
use alloc::format;
use api::task::TaskPriority;
use core::hint::black_box;
use ostd::{
    io::println,
    syscall::{
        sys_exit, sys_force_exit, sys_get_time, sys_heartbeat, sys_notify_on_exit, sys_recv,
        sys_send, sys_set_spawn_args, sys_spawn_pinned, SyscallResult,
    },
    task::yield_now,
};

/// Iterations per worker run.  Calibrated for ≥1 ms on QEMU TCG.
pub const SMP_WORKER_ITERS: u64 = 500_000;

const PROBE_PATH: &str = "/bin/bench-probe";
const CAVEAT: &str = " [QEMU-TCG: 2 hart-threads; real-HW shows true 2× parallelism]";

/// Optimizer-resistant arithmetic workload shared by worker and orchestrator.
fn compute(iters: u64) -> u64 {
    let mut acc = 0u64;
    for i in 0..iters {
        acc = acc.wrapping_add(i.wrapping_mul(3));
    }
    black_box(acc)
}

/// `smp-worker` bench-probe role entry — runs compute, prints, exits.
pub fn run_worker() -> ! {
    let acc = compute(SMP_WORKER_ITERS);
    println(&format!("[smp] worker done acc={}", acc));
    sys_exit(0);
}

/// Wait for one start message, then deliberately miss a heartbeat deadline.
/// A caller's next blocking send must resume with an error when the watchdog
/// kills this peer through the ordinary `exit_task` path.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_heartbeat_peer() -> ! {
    let mut start = [0u8; 1];
    let _ = sys_recv(0, &mut start);
    sys_heartbeat(5);
    loop {
        yield_now();
    }
}

// ── Orchestrator-only helpers ─────────────────────────────────────────────────

fn spawn_worker() -> Result<usize, ()> {
    if !sys_set_spawn_args("smp-worker") {
        return Err(());
    }
    match sys_spawn_pinned(PROBE_PATH, TaskPriority::Normal as u8, 0) {
        SyscallResult::Ok(tid) if tid != 0 => Ok(tid),
        _ => Err(()),
    }
}

fn recv_exit(tid: usize) -> Result<(), ()> {
    let mut buf = [0u8; 8];
    match sys_recv(tid, &mut buf) {
        SyscallResult::Ok(sender) if sender == tid => Ok(()),
        _ => Err(()),
    }
}

fn wait_exit(tid: usize) -> Result<(), ()> {
    if !matches!(sys_notify_on_exit(tid), SyscallResult::Ok(_)) {
        return Err(());
    }
    recv_exit(tid)
}

fn force_exit(tid: usize) -> bool {
    matches!(sys_force_exit(tid), SyscallResult::Ok(_))
}

/// Prove caller-visible dead-peer error plus queued ForceExit notification.
pub fn run_peer_death_guard() -> ! {
    if !sys_set_spawn_args("heartbeat-peer") {
        println("[peer-death-runtime] FAIL — heartbeat child argument staging");
        sys_exit(1);
    }
    let blocked_peer = match sys_spawn_pinned(PROBE_PATH, TaskPriority::Normal as u8, 0) {
        SyscallResult::Ok(tid) => tid,
        _ => {
            println("[peer-death-runtime] FAIL — heartbeat child spawn");
            sys_exit(1);
        }
    };
    for _ in 0..20 {
        yield_now();
    }
    if !matches!(sys_send(blocked_peer, &[1]), SyscallResult::Ok(_)) {
        println("[peer-death-runtime] FAIL — heartbeat child start");
        let _ = sys_force_exit(blocked_peer);
        sys_exit(1);
    }
    if !matches!(sys_send(blocked_peer, &[2]), SyscallResult::Err(_)) {
        println("[peer-death-runtime] FAIL — blocked send did not return an error");
        let _ = sys_force_exit(blocked_peer);
        sys_exit(1);
    }

    if !sys_set_spawn_args("load") {
        println("[peer-death-runtime] FAIL — force-exit child argument staging");
        sys_exit(1);
    }
    let forced_tid = match sys_spawn_pinned(PROBE_PATH, TaskPriority::Normal as u8, 0) {
        SyscallResult::Ok(tid) => tid,
        _ => {
            println("[peer-death-runtime] FAIL — ForceExit child spawn");
            sys_exit(1);
        }
    };
    if !matches!(sys_notify_on_exit(forced_tid), SyscallResult::Ok(_)) {
        println("[peer-death-runtime] FAIL — ForceExit watch");
        let _ = sys_force_exit(forced_tid);
        sys_exit(1);
    }
    for _ in 0..20 {
        yield_now();
    }
    if !matches!(sys_force_exit(forced_tid), SyscallResult::Ok(_)) {
        println("[peer-death-runtime] FAIL — ForceExit denied");
        sys_exit(1);
    }
    let _ = recv_exit(forced_tid);

    println("[peer-death-runtime] PASS (blocked-send error + ForceExit notification)");
    sys_exit(0);
}

// ── Scenario results ──────────────────────────────────────────────────────────

pub struct SmpMetric {
    pub name: &'static str,
    pub n: u32,
    pub value: u64,
    pub passed: bool,
}

pub struct SmpFailure {
    pub name: &'static str,
    pub stage: &'static str,
    pub teardown_failed: bool,
}

pub enum SmpOutcome {
    Metric(SmpMetric),
    Invalid(SmpFailure),
}

fn invalid(name: &'static str, stage: &'static str, teardown_failed: bool) -> SmpOutcome {
    SmpOutcome::Invalid(SmpFailure {
        name,
        stage,
        teardown_failed,
    })
}

// ── Scenario 1: spawn_rate ────────────────────────────────────────────────────

fn measure_spawn_rate() -> SmpOutcome {
    const N: u64 = 8;
    const TARGET: u64 = 10;
    const NAME: &str = "smp_spawn_rate";

    let t0 = sys_get_time();
    for _ in 0..N {
        let tid = match spawn_worker() {
            Ok(tid) => tid,
            Err(()) => return invalid(NAME, "setup", false),
        };
        if wait_exit(tid).is_err() {
            return invalid(NAME, "measure", !force_exit(tid));
        }
    }
    let dt_ns = sys_get_time()
        .saturating_sub(t0)
        .saturating_mul(NS_PER_TICK);
    if dt_ns == 0 {
        return invalid(NAME, "measure", false);
    }
    let per_sec = N.saturating_mul(1_000_000_000) / dt_ns;
    let passed = per_sec >= TARGET;
    println(&format!(
        "[smp] spawn_rate {}: {}/sec (target ≥{}/sec){}",
        if passed { "PASS" } else { "FAIL" },
        per_sec,
        TARGET,
        CAVEAT
    ));
    SmpOutcome::Metric(SmpMetric {
        name: NAME,
        n: N as u32,
        value: per_sec,
        passed,
    })
}

// ── Scenario 2: ipc_throughput ────────────────────────────────────────────────

fn measure_ipc_throughput() -> SmpOutcome {
    const MSGS: u64 = 1_000;
    const TARGET: u64 = 5_000;
    const NAME: &str = "smp_ipc_throughput";

    if !sys_set_spawn_args("ipc-echo") {
        return invalid(NAME, "setup", false);
    }
    let echo_tid = match sys_spawn_pinned(PROBE_PATH, TaskPriority::Normal as u8, 0) {
        SyscallResult::Ok(tid) if tid != 0 => tid,
        _ => return invalid(NAME, "setup", false),
    };
    for _ in 0..20 {
        yield_now();
    }

    let mut request = [0u8; 64];
    request[0] = 0x42;
    let mut reply = [0u8; 8];
    let t0 = sys_get_time();
    let mut operation_failed = false;
    for _ in 0..MSGS {
        reply.fill(0xa5);
        if !matches!(sys_send(echo_tid, &request), SyscallResult::Ok(_)) {
            operation_failed = true;
            break;
        }
        match sys_recv(echo_tid, &mut reply) {
            SyscallResult::Ok(sender) if sender == echo_tid => {}
            _ => {
                operation_failed = true;
                break;
            }
        }
        if reply[0] != 0 || reply[1..].iter().any(|&byte| byte != 0xa5) {
            operation_failed = true;
            break;
        }
    }
    let dt_ns = sys_get_time()
        .saturating_sub(t0)
        .saturating_mul(NS_PER_TICK);
    let teardown_failed = !force_exit(echo_tid);
    if operation_failed || dt_ns == 0 {
        return invalid(NAME, "measure", teardown_failed);
    }
    if teardown_failed {
        return invalid(NAME, "teardown", false);
    }

    let per_sec = MSGS.saturating_mul(1_000_000_000) / dt_ns;
    let passed = per_sec >= TARGET;
    println(&format!(
        "[smp] ipc_throughput {}: {}/sec (target ≥{}/sec){}",
        if passed { "PASS" } else { "FAIL" },
        per_sec,
        TARGET,
        CAVEAT
    ));
    SmpOutcome::Metric(SmpMetric {
        name: NAME,
        n: MSGS as u32,
        value: per_sec,
        passed,
    })
}

// ── Scenario 3: work_distribution ─────────────────────────────────────────────

fn measure_work_distribution() -> SmpOutcome {
    const NAME: &str = "smp_work_distribution";

    let t0 = sys_get_time();
    let tid1 = match spawn_worker() {
        Ok(tid) => tid,
        Err(()) => return invalid(NAME, "setup", false),
    };
    if wait_exit(tid1).is_err() {
        return invalid(NAME, "measure", !force_exit(tid1));
    }
    let t_single = sys_get_time().saturating_sub(t0);

    let t1 = sys_get_time();
    let tid2 = match spawn_worker() {
        Ok(tid) => tid,
        Err(()) => return invalid(NAME, "setup", false),
    };
    if !matches!(sys_notify_on_exit(tid2), SyscallResult::Ok(_)) {
        return invalid(NAME, "measure", !force_exit(tid2));
    }
    let _ = compute(SMP_WORKER_ITERS);
    if recv_exit(tid2).is_err() {
        return invalid(NAME, "measure", !force_exit(tid2));
    }
    let t_parallel = sys_get_time().saturating_sub(t1);
    if t_single == 0 || t_parallel == 0 {
        return invalid(NAME, "measure", false);
    }

    let scale_x100 = t_single.saturating_mul(200) / t_parallel;
    let passed = scale_x100 >= 140;
    println(&format!(
        "[smp] work_distribution {}: scale={}.{:02}x T1={}t Tp={}t (target ≥1.40x){}",
        if passed { "PASS" } else { "FAIL" },
        scale_x100 / 100,
        scale_x100 % 100,
        t_single,
        t_parallel,
        CAVEAT
    ));
    SmpOutcome::Metric(SmpMetric {
        name: NAME,
        n: 2,
        value: scale_x100,
        passed,
    })
}

// ── Suite entry ───────────────────────────────────────────────────────────────

pub fn run_smp_suite() -> [SmpOutcome; 3] {
    [
        measure_spawn_rate(),
        measure_ipc_throughput(),
        measure_work_distribution(),
    ]
}
