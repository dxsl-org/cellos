//! Phase-03 step-1 prototype: bounded **multi-outstanding** local calls over the
//! shipped exact-operation IPC, measured on the Phase-02 lifecycle image.
//!
//! Phase 03 has to decide between a userland scheduler over the primitive that
//! already exists and a new kernel submission/completion source — and a new public
//! source needs two Law-1 confirmations. This module is the measurement that decision
//! rests on, and it changes nothing: no ABI, no kernel path, no production callsite.
//!
//! Three legs, one marker per leg:
//!
//! | Leg | Question it answers with a number |
//! |---|---|
//! | 1 | Can one Cell hold N outstanding bounded calls against a real peer, get **exactly one** correlated completion for each, and at what latency and poll cost? |
//! | 2 | Is `sys_try_send` a submission mechanism, or is it dropped unless the receiver is parked in `Recv`? |
//! | 3 | Does a peer that dies mid-flight terminalise **every** outstanding operation, with none lost? |
//!
//! The stranding baseline this work exists to fix is measured in
//! `scenarios::local_service_lifecycle` (synchronous leg: the caller is never woken)
//! and is not duplicated here.

use alloc::format;
use alloc::vec::Vec;
use api::ipc::IPC_BUF_SIZE;
use api::task::TaskPriority;
use ostd::io::println;
use ostd::ipc::{submit, take, wait, IpcTakeResult, IpcTerminal};
use ostd::syscall::{
    sys_exit, sys_get_time, sys_ipc_current, sys_ipc_reply, sys_recv, sys_send, sys_set_spawn_args,
    sys_spawn_pinned, sys_try_send, SyscallResult,
};
use ostd::task::yield_now;

const PROBE_PATH: &str = "/bin/bench-probe";
const ECHO_PREFIX: &str = "c2c-echo-reverse:";
const SLOW_ROLE: &str = "c2c-slow-parked";
/// The existing role: consume one tagged request, then exit **without** replying.
const VANISH_ROLE: &str = "c2c-provider";

/// First byte of every request, and of the echo reply.
const REQ_TAG: u8 = 0x5A;
const REPLY_TAG: u8 = 0x5B;
/// Sequence a caller sends on the ordinary path to release a probe that must outlive
/// the bounded operations it answered.
const DONE_SEQ: u8 = 0xFF;

/// Yields allowed for a spawned probe to reach its parked receive.
const SETTLE_YIELDS: usize = 60;
/// Outstanding calls in the bounded leg.
const OUTSTANDING: usize = 8;
/// Outstanding calls in the mid-flight-death leg.
const DEATH_OUTSTANDING: usize = 4;
/// Per-`wait` tick budget (10 ms ticks) and the rounds the collector allows.
const WAIT_TICKS: u64 = 200;
const WAIT_ROUNDS: usize = 40;
/// How long the slow provider stays out of `Recv` (long enough for two try_sends).
const SLOW_SLEEP_YIELDS: usize = 1200;

fn fail(reason: &str) -> ! {
    println(&format!("[async-lifecycle] FAIL — {reason}"));
    sys_exit(1)
}

fn spawn(role: &str) -> usize {
    if !sys_set_spawn_args(role) {
        fail("spawn args");
    }
    match sys_spawn_pinned(PROBE_PATH, TaskPriority::Normal as u8, 0) {
        SyscallResult::Ok(tid) if tid != 0 => tid,
        _ => fail("spawn"),
    }
}

fn settle() {
    for _ in 0..SETTLE_YIELDS {
        yield_now();
    }
}

// ── probe roles ───────────────────────────────────────────────────────────────

/// Echo provider: consume exactly `n` tagged requests, then answer them **in reverse
/// order**, so a caller that assumes FIFO correlation cannot pass, and print one line
/// per reply so the log shows the order they left in.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_echo_reverse(role: &str) -> ! {
    let count = role
        .strip_prefix(ECHO_PREFIX)
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(0);
    println(&format!(
        "[async-lifecycle] echo-provider-ready count={count}"
    ));
    let mut buf = [0u8; IPC_BUF_SIZE];
    let mut pending: Vec<(usize, u8)> = Vec::new();
    while pending.len() < count {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(_) if buf[0] == REQ_TAG && buf[1] != DONE_SEQ => {
                // The kernel records the exact operation this message belongs to;
                // a bounded reply must name it, not just the sender.
                let operation = sys_ipc_current();
                if operation <= 0 {
                    fail("echo provider: no operation id for a bounded request");
                }
                pending.push((operation as usize, buf[1]));
            }
            SyscallResult::Ok(_) => { /* not ours: keep waiting */ }
            SyscallResult::Err(_) => fail("echo provider recv"),
        }
    }
    for (operation, seq) in pending.iter().rev() {
        if sys_ipc_reply(*operation, &[REPLY_TAG, *seq]) < 0 {
            fail("echo provider bounded reply");
        }
    }
    println(&format!(
        "[async-lifecycle] echo-provider-replied count={count} reversed"
    ));

    // Stay alive until the caller releases us: exiting here would terminalise the
    // operations the caller has not taken yet as peer-gone, which is leg 3's shape,
    // not this leg's.
    loop {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(_) if buf[0] == REQ_TAG && buf[1] == DONE_SEQ => {
                println("[async-lifecycle] echo-provider-done");
                sys_exit(0)
            }
            SyscallResult::Ok(_) => continue,
            SyscallResult::Err(_) => fail("echo provider done-recv"),
        }
    }
}

/// Parked provider: consume one request, stay busy for a bounded while, then take a
/// second receive. What that second receive finds is the measurement: a frame the
/// caller's `sys_try_send` accepted while this cell was *not* in `Recv` is either
/// queued for us or silently dropped.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_slow_parked() -> ! {
    println("[async-lifecycle] slow-provider-ready");
    let mut buf = [0u8; IPC_BUF_SIZE];
    let _sender = match sys_recv(0, &mut buf) {
        SyscallResult::Ok(from) => from,
        SyscallResult::Err(_) => fail("slow provider recv"),
    };
    println("[async-lifecycle] slow-provider-busy");
    for _ in 0..SLOW_SLEEP_YIELDS {
        yield_now();
    }
    // The caller releases us either way, so drain until that release arrives:
    // anything else found first is a frame the kernel accepted while we were not
    // in `Recv`, i.e. queued rather than dropped.
    let mut queued = false;
    loop {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(_) if buf[0] == REQ_TAG && buf[1] == DONE_SEQ => break,
            SyscallResult::Ok(_) => queued = true,
            SyscallResult::Err(_) => {
                println("[async-lifecycle] slow-provider-second=error");
                break;
            }
        }
    }
    println(&format!(
        "[async-lifecycle] slow-provider-second queued={queued}"
    ));
    println("[async-lifecycle] slow-provider-done");
    sys_exit(0)
}

// ── orchestrator legs ─────────────────────────────────────────────────────────

/// Leg 1: N outstanding bounded calls, exactly one correlated completion each.
fn bounded_multi_outstanding_leg() {
    let provider = spawn(&format!("{ECHO_PREFIX}{OUTSTANDING}"));
    settle();

    let mut tokens: Vec<usize> = Vec::new();
    let mut submitted_at: Vec<u64> = Vec::new();
    for seq in 0..OUTSTANDING {
        let request = [REQ_TAG, seq as u8];
        match submit(provider, &request) {
            Ok(token) => {
                tokens.push(token);
                submitted_at.push(sys_get_time());
            }
            Err(error) => {
                println(&format!(
                    "[async-lifecycle] BOUNDED-SUBMIT-FAILED {error:?}"
                ));
                fail("a bounded submit was refused with the peer parked");
            }
        }
    }

    let mut done = alloc::vec![false; tokens.len()];
    let mut completed = 0usize;
    let mut lost = 0usize;
    let mut wrong_seq = 0usize;
    let mut latencies: Vec<u64> = Vec::new();
    let mut rounds_used = 0usize;
    let mut reply = [0u8; IPC_BUF_SIZE];

    while completed + lost < tokens.len() && rounds_used < WAIT_ROUNDS {
        rounds_used += 1;
        if !wait(WAIT_TICKS) {
            continue;
        }
        for index in 0..tokens.len() {
            if done[index] {
                continue;
            }
            match take(tokens[index], &mut reply) {
                Ok(IpcTakeResult::Pending) => {}
                Ok(IpcTakeResult::Terminal { status, len }) => {
                    done[index] = true;
                    if !matches!(status, IpcTerminal::Reply) {
                        println(&format!("[async-lifecycle] BOUNDED-STATUS {status:?}"));
                        lost += 1;
                        continue;
                    }
                    // Correlation is the token, not the arrival order: the provider
                    // answers in reverse, and the payload must still carry this
                    // operation's own sequence number.
                    if len < 2 || reply[0] != REPLY_TAG || reply[1] as usize != index {
                        wrong_seq += 1;
                    }
                    latencies.push(sys_get_time().saturating_sub(submitted_at[index]));
                    completed += 1;
                }
                Err(error) => {
                    println(&format!("[async-lifecycle] BOUNDED-TAKE-ERR {error:?}"));
                    done[index] = true;
                    lost += 1;
                }
            }
        }
    }
    // Anything still pending after the budget is a completion that never arrived.
    lost += tokens.len() - completed - lost;

    latencies.sort_unstable();
    let p50 = latencies.get(latencies.len() / 2).copied().unwrap_or(0);
    let p99 = latencies
        .get(latencies.len().saturating_sub(1))
        .copied()
        .unwrap_or(0);
    let freq = ostd::syscall::sys_get_timer_freq().unwrap_or(0);
    println(&format!(
        "[async-lifecycle] BOUNDED completed={} lost={} wrong_seq={} p50_ticks={} p99_ticks={} freq_hz={} wait_rounds={}",
        completed,
        lost,
        wrong_seq,
        p50,
        p99,
        freq,
        rounds_used
    ));
    let released = matches!(
        sys_send(provider, &[REQ_TAG, DONE_SEQ]),
        SyscallResult::Ok(_)
    );
    println(&format!("[async-lifecycle] BOUNDED-RELEASED ok={released}"));
    if completed != OUTSTANDING || lost != 0 || wrong_seq != 0 {
        fail("bounded multi-outstanding leg did not complete exactly once per request");
    }
}

/// Leg 2: is `sys_try_send` a submission mechanism?
///
/// `TrySend` refuses a non-blocking delivery **in its return value** — the kernel
/// answers `isize::MAX` and the SDK wrapper hands that back as `Ok`, so a caller must
/// compare the value rather than the `Result`. This leg measures the rule: one send
/// to a parked receiver, one while the receiver is busy, and the provider's own
/// second receive saying whether anything the kernel accepted was queued.
fn try_send_leg() {
    /// The kernel's "dropped, target not receiving" sentinel, **as the SDK wrapper
    /// hands it back**: `usize::MAX`. It arrives as a value, not as an `Err`, so a
    /// caller that checks only the `Result` reads a dropped frame as a delivered one.
    const REFUSED: usize = usize::MAX;

    let provider = spawn(SLOW_ROLE);
    settle();

    let parked_ret = value_of(sys_try_send(provider, &[REQ_TAG, 1]));

    // Give the provider time to consume that frame and leave `Recv`.
    for _ in 0..200 {
        yield_now();
    }
    let busy_ret = value_of(sys_try_send(provider, &[REQ_TAG, 2]));

    // Release the provider so its verdict is bounded, then let it run.
    let _ = sys_send(provider, &[REQ_TAG, DONE_SEQ]);
    for _ in 0..SLOW_SLEEP_YIELDS {
        yield_now();
    }

    let parked_refused = parked_ret == REFUSED;
    let busy_refused = busy_ret == REFUSED;
    println(&format!(
        "[async-lifecycle] TRY-SEND parked_refused={parked_refused} busy_refused={busy_refused} parked_ret={parked_ret} busy_ret={busy_ret}"
    ));
    if parked_refused {
        fail("try_send refused a frame to a provider parked in Recv");
    }
    if !busy_refused {
        fail("try_send accepted a frame for a receiver that was not in Recv");
    }
}

/// The syscall's return value, whichever arm it came back on.
fn value_of(result: SyscallResult) -> usize {
    match result {
        SyscallResult::Ok(value) => value,
        SyscallResult::Err(_) => usize::MAX,
    }
}

/// Leg 3: a peer that dies mid-flight terminalises every outstanding operation.
fn mid_flight_death_leg() {
    let provider = spawn(VANISH_ROLE);
    settle();

    let mut tokens: Vec<usize> = Vec::new();
    let mut submit_refused = 0usize;
    for seq in 0..DEATH_OUTSTANDING {
        match submit(provider, &[REQ_TAG, seq as u8]) {
            Ok(token) => tokens.push(token),
            // A pre-dispatch refusal is also a definite outcome, and must not be
            // counted as a lost operation.
            Err(_) => submit_refused += 1,
        }
    }

    let mut done = alloc::vec![false; tokens.len()];
    let mut terminals = 0usize;
    let mut peer_gone = 0usize;
    let mut lost = 0usize;
    let mut reply = [0u8; IPC_BUF_SIZE];
    let mut rounds_used = 0usize;
    while terminals < tokens.len() && rounds_used < WAIT_ROUNDS {
        rounds_used += 1;
        if !wait(WAIT_TICKS) {
            continue;
        }
        for index in 0..tokens.len() {
            if done[index] {
                continue;
            }
            match take(tokens[index], &mut reply) {
                Ok(IpcTakeResult::Pending) => {}
                Ok(IpcTakeResult::Terminal { status, .. }) => {
                    done[index] = true;
                    terminals += 1;
                    if matches!(status, IpcTerminal::PeerGone) {
                        peer_gone += 1;
                    } else {
                        println(&format!("[async-lifecycle] DEATH-STATUS {status:?}"));
                    }
                }
                Err(error) => {
                    println(&format!("[async-lifecycle] DEATH-TAKE-ERR {error:?}"));
                    done[index] = true;
                    terminals += 1;
                    lost += 1;
                }
            }
        }
    }
    let unterminal = tokens.len() - terminals;
    println(&format!(
        "[async-lifecycle] DEATH outstanding={} submit_refused={} terminal={} peer_gone={} unterminal={} lost={}",
        tokens.len(),
        submit_refused,
        terminals,
        peer_gone,
        unterminal,
        lost
    ));
    if unterminal != 0 || lost != 0 || terminals == 0 {
        fail("a dead peer left an outstanding operation without a terminal outcome");
    }
}

pub fn run() -> ! {
    println("[async-lifecycle] START");
    bounded_multi_outstanding_leg();
    try_send_leg();
    mid_flight_death_leg();
    println("[async-lifecycle] PASS");
    sys_exit(0)
}
