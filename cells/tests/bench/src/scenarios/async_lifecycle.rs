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

use abi::task::TaskPriority;
use alloc::format;
use alloc::vec::Vec;
use ostd::io::println;
use ostd::ipc::{current, reply, wait, wait_any, PendingCall, Wake};
use ostd::syscall::{
    sys_exit, sys_get_scheduler_ticks, sys_get_time, sys_notify_on_exit, sys_recv, sys_send,
    sys_set_spawn_args, sys_spawn_pinned, sys_try_send, SyscallResult,
};
use ostd::task::yield_now;
use wire::ipc::IPC_BUF_SIZE;

#[cfg(target_arch = "x86_64")]
use abi::completion::source::IRQ;
use abi::completion::source::{IPC, NET_RX, TIMER};

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
                // The kernel records the exact operation this message belongs to; a
                // bounded reply must name it, not just the sender.
                match current() {
                    Some(operation) => pending.push((operation, buf[1])),
                    None => fail("echo provider: no operation id for a bounded request"),
                }
            }
            SyscallResult::Ok(_) => { /* not ours: keep waiting */ }
            SyscallResult::Err(_) => fail("echo provider recv"),
        }
    }
    for (operation, seq) in pending.iter().rev() {
        if reply(*operation, &[REPLY_TAG, *seq]).is_err() {
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

    let mut calls: Vec<PendingCall> = Vec::new();
    let mut submitted_at: Vec<u64> = Vec::new();
    for seq in 0..OUTSTANDING {
        let request = [REQ_TAG, seq as u8];
        match PendingCall::submit(provider, &request) {
            Ok(call) => {
                calls.push(call);
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

    let mut done = alloc::vec![false; calls.len()];
    let mut completed = 0usize;
    let mut lost = 0usize;
    let mut wrong_seq = 0usize;
    let mut latencies: Vec<u64> = Vec::new();
    let mut rounds_used = 0usize;
    let mut reply = [0u8; IPC_BUF_SIZE];

    while completed + lost < calls.len() && rounds_used < WAIT_ROUNDS {
        rounds_used += 1;
        if !wait(WAIT_TICKS) {
            continue;
        }
        for index in 0..calls.len() {
            if done[index] {
                continue;
            }
            match calls[index].try_take(&mut reply) {
                Ok(None) => {}
                Ok(Some(completion)) => {
                    done[index] = true;
                    if !completion.is_definite() || completion.len < 2 {
                        println(&format!(
                            "[async-lifecycle] BOUNDED-COMPLETION {completion:?}"
                        ));
                        lost += 1;
                        continue;
                    }
                    // Correlation is the token, not the arrival order: the provider
                    // answers in reverse, and the payload must still carry this
                    // operation's own sequence number.
                    if completion.len < 2 || reply[0] != REPLY_TAG || reply[1] as usize != index {
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
    lost += calls.len() - completed - lost;

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
    /// How long the parked send may keep retrying before the provider counts as
    /// never parked (scheduler ticks, about 5 s at the 100 Hz tick).
    const PARK_WINDOW_TICKS: u64 = 500;

    let provider = spawn(SLOW_ROLE);
    settle();

    // The provider parks in `Recv` only after its ready line reaches the UART, and
    // on SMP it runs on another CPU, so no fixed number of yields here guarantees it
    // is parked. A refusal while it is still running is correct and drops the frame
    // (nothing is queued), so retry until it is accepted, within a bounded window.
    let park_deadline = sys_get_scheduler_ticks().unwrap_or(0) + PARK_WINDOW_TICKS;
    let mut park_attempts = 1usize;
    let mut parked_ret = value_of(sys_try_send(provider, &[REQ_TAG, 1]));
    while parked_ret == REFUSED && sys_get_scheduler_ticks().unwrap_or(u64::MAX) < park_deadline {
        yield_now();
        park_attempts += 1;
        parked_ret = value_of(sys_try_send(provider, &[REQ_TAG, 1]));
    }

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
        "[async-lifecycle] TRY-SEND parked_refused={parked_refused} busy_refused={busy_refused} parked_ret={parked_ret} busy_ret={busy_ret} park_attempts={park_attempts}"
    ));
    if parked_refused {
        fail("try_send refused every frame to the slow provider within the park window");
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

    let mut calls: Vec<PendingCall> = Vec::new();
    let mut submit_refused = 0usize;
    for seq in 0..DEATH_OUTSTANDING {
        match PendingCall::submit(provider, &[REQ_TAG, seq as u8]) {
            Ok(call) => calls.push(call),
            // A pre-dispatch refusal is also a definite outcome, and must not be
            // counted as a lost operation.
            Err(_) => submit_refused += 1,
        }
    }

    let mut done = alloc::vec![false; calls.len()];
    let mut terminals = 0usize;
    let mut peer_gone = 0usize;
    let mut lost = 0usize;
    let mut reply = [0u8; IPC_BUF_SIZE];
    let mut rounds_used = 0usize;
    while terminals < calls.len() && rounds_used < WAIT_ROUNDS {
        rounds_used += 1;
        if !wait(WAIT_TICKS) {
            continue;
        }
        for index in 0..calls.len() {
            if done[index] {
                continue;
            }
            match calls[index].try_take(&mut reply) {
                Ok(None) => {}
                Ok(Some(completion)) => {
                    done[index] = true;
                    terminals += 1;
                    if completion.terminal == ostd::ipc::IpcTerminal::PeerGone {
                        peer_gone += 1;
                    } else {
                        fail("dead peer produced a terminal other than PeerGone");
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
    let unterminal = calls.len() - terminals;
    println(&format!(
        "[async-lifecycle] DEATH outstanding={} submit_refused={} terminal={} peer_gone={} unterminal={} lost={}",
        calls.len(),
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
    multi_source_leg();
    cancel_race_leg();
    sync_timeout_late_leg();
    drop_reclaim_leg();
    println("[async-lifecycle] PASS");
    sys_exit(0)
}

// ── Phase 03d: multi-source `WaitCompletion` (IPC + TIMER + NET_RX) ──────────
//
// The caller runs in `bench-probe`, whose allowlist holds `WaitCompletion`
// (bit 42); `bench` itself does not, so legs 1-3 above exercise the SDK's
// `IpcWait` path while these legs exercise the completion path directly.

const MSW_CALLER: &str = "c2c-msw-caller";
const MSW_ECHO: &str = "c2c-msw-echo";
const MSW_SLOW: &str = "c2c-msw-slow";
const MSW_HOLD: &str = "c2c-msw-hold";
const MSW_RAW: &str = "c2c-msw-raw";
/// Control frames between the orchestrator, the caller and the hold provider.
const CTRL_TAG: u8 = 0x5D;
/// A raw (non-RPC) mailbox message.
const RAW_TAG: u8 = 0x5E;
/// M5 rounds per wait mechanism.
const M5_ROUNDS: usize = 200;
/// Non-starvation load: fast provider A, slow provider B.
const A_OUTSTANDING: usize = 24;
const B_OUTSTANDING: usize = 4;
const A_TARGET: usize = 600;
const B_TARGET: usize = 8;
const SLOW_REPLY_YIELDS: usize = 50;
/// The kernel's per-operation expiry (`async_ipc::OPERATION_TIMEOUT_TICKS`).
const OPERATION_TIMEOUT_TICKS: u64 = 3_000;
/// Fairness bound stated by CP1 §5.5 for one waiter.
const MAX_OVERTAKES: usize = 63;

fn msw_fail(reason: &str) -> ! {
    println(&format!("[async-lifecycle] FAIL — multi-source: {reason}"));
    sys_exit(1)
}

fn ticks() -> u64 {
    sys_get_scheduler_ticks().unwrap_or_else(|| msw_fail("scheduler ticks"))
}

fn submit_or_fail(peer: usize, request: &[u8]) -> PendingCall {
    PendingCall::submit(peer, request).unwrap_or_else(|_| msw_fail("submit"))
}

fn take_or_fail(call: &PendingCall, reply: &mut [u8]) -> ostd::ipc::Completion {
    match call.try_take(reply) {
        Ok(Some(completion)) => completion,
        Ok(None) => msw_fail("a reported operation had no terminal to take"),
        Err(_) => msw_fail("take"),
    }
}

/// Fast provider: answer each bounded request at once; `[REQ_TAG, DONE_SEQ]`
/// (a raw message) exits.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_msw_echo() -> ! {
    let mut buf = [0u8; IPC_BUF_SIZE];
    loop {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(_) if buf[0] == REQ_TAG && buf[1] == DONE_SEQ => sys_exit(0),
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(_) => msw_fail("echo recv"),
        }
        let Some(operation) = current() else {
            continue;
        };
        if reply(operation, &[REPLY_TAG, buf[1]]).is_err() {
            msw_fail("echo reply");
        }
    }
}

/// Raw event source with no RPC context: on each raw `[CTRL_TAG]` from the
/// caller it sends one raw `[RAW_TAG]` back. A provider serving the caller's
/// operation cannot do this: a Send to the currently served caller is that
/// operation's reply (Spec 20 §2.6 immediate-reply bridge).
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_msw_raw() -> ! {
    let mut buf = [0u8; IPC_BUF_SIZE];
    loop {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(_) if buf[0] == REQ_TAG && buf[1] == DONE_SEQ => sys_exit(0),
            SyscallResult::Ok(sender) if buf[0] == CTRL_TAG => {
                for _ in 0..SETTLE_YIELDS {
                    yield_now();
                }
                let _ = sys_send(sender, &[RAW_TAG]);
            }
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(_) => msw_fail("raw source recv"),
        }
    }
}

/// Slow provider: answer each bounded request after a fixed busy period, and
/// stamp the reply with the time it was sent.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_msw_slow() -> ! {
    let mut buf = [0u8; IPC_BUF_SIZE];
    loop {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(_) if buf[0] == REQ_TAG && buf[1] == DONE_SEQ => sys_exit(0),
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(_) => msw_fail("slow recv"),
        }
        let Some(operation) = current() else {
            continue;
        };
        for _ in 0..SLOW_REPLY_YIELDS {
            yield_now();
        }
        let mut out = [0u8; 10];
        out[0] = REPLY_TAG;
        out[1] = buf[1];
        out[2..10].copy_from_slice(&sys_get_time().to_le_bytes());
        if reply(operation, &out).is_err() {
            msw_fail("slow reply");
        }
    }
}

/// Hold provider: keep the first request dispatched and unanswered until the
/// caller reports its expiry, then try the late reply (it must be refused),
/// answer one fresh request with that verdict, and exit on `DONE`.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_msw_hold() -> ! {
    let mut buf = [0u8; IPC_BUF_SIZE];
    let held = loop {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(_) if buf[0] == REQ_TAG => match current() {
                Some(operation) => break operation,
                None => msw_fail("hold token"),
            },
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(_) => msw_fail("hold recv"),
        }
    };
    loop {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(_) if buf[0] == CTRL_TAG => break,
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(_) => msw_fail("hold control"),
        }
    }
    let late_refused = reply(held, &[REPLY_TAG, 0xEE]).is_err();
    loop {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(_) if buf[0] == REQ_TAG && buf[1] == DONE_SEQ => sys_exit(0),
            SyscallResult::Ok(_) if buf[0] == REQ_TAG => {
                let fresh = current().unwrap_or_else(|| msw_fail("hold fresh token"));
                if fresh == held || reply(fresh, &[REPLY_TAG, late_refused as u8]).is_err() {
                    msw_fail("hold fresh reply");
                }
            }
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(_) => msw_fail("hold drain"),
        }
    }
}

/// The multi-source caller. Receives the provider tids from the orchestrator,
/// runs every leg, then reports `[CTRL_TAG, 1]` to the orchestrator.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_msw_caller() -> ! {
    let mut buf = [0u8; IPC_BUF_SIZE];
    let parent = match sys_recv(0, &mut buf) {
        SyscallResult::Ok(sender) if buf[0] == CTRL_TAG => sender,
        _ => msw_fail("caller start"),
    };
    let tid = |index: usize| {
        let at = 1 + 8 * index;
        u64::from_le_bytes(buf[at..at + 8].try_into().unwrap()) as usize
    };
    let (echo, slow, hold, vanish, echo_reverse, raw) =
        (tid(0), tid(1), tid(2), tid(3), tid(4), tid(5));
    msw_reply_leg(echo_reverse);
    msw_m5_leg(echo);
    msw_mixed_leg(echo, raw);
    msw_peer_death_leg(vanish);
    msw_starvation_leg(echo, slow);
    msw_deadline_and_stale_leg(hold);
    for provider in [echo, slow, raw] {
        let _ = sys_send(provider, &[REQ_TAG, DONE_SEQ]);
    }
    let _ = sys_send(parent, &[CTRL_TAG, 1]);
    sys_exit(0)
}

/// A1: eight outstanding calls, one IPC record each, nothing re-reported while
/// every result stays retained.
fn msw_reply_leg(provider: usize) {
    let calls: Vec<PendingCall> = (0..OUTSTANDING)
        .map(|seq| submit_or_fail(provider, &[REQ_TAG, seq as u8]))
        .collect();
    let mut seen = alloc::vec![false; calls.len()];
    let (mut records, mut dup, mut foreign, mut timer_returns) = (0usize, 0usize, 0usize, 0usize);
    let deadline = ticks() + 2_000;
    while records < calls.len() && ticks() < deadline {
        match wait_any(IPC | TIMER, 200) {
            Ok(Wake::Ipc(op)) => match calls.iter().position(|call| call.operation() == op) {
                Some(index) if seen[index] => dup += 1,
                Some(index) => {
                    seen[index] = true;
                    records += 1;
                }
                None => foreign += 1,
            },
            Ok(Wake::Timer) => timer_returns += 1,
            _ => msw_fail("reply leg wait"),
        }
    }
    // Every result is still retained: a one-shot source must stay quiet.
    let rereport = match wait_any(IPC | TIMER, 5) {
        Ok(Wake::Timer) => 0,
        Ok(Wake::Ipc(_)) => 1,
        _ => msw_fail("reply leg quiet check"),
    };
    let mut reply_buf = [0u8; IPC_BUF_SIZE];
    let mut wrong_seq = 0usize;
    for (index, call) in calls.iter().enumerate() {
        let completion = take_or_fail(call, &mut reply_buf);
        if !completion.is_definite() || reply_buf[..2] != [REPLY_TAG, index as u8] {
            wrong_seq += 1;
        }
    }
    let _ = sys_send(provider, &[REQ_TAG, DONE_SEQ]);
    println(&format!(
        "[async-lifecycle] MSW-REPLY records={records} dup={dup} foreign={foreign} \
         timer_returns={timer_returns} rereport={rereport} wrong_seq={wrong_seq}"
    ));
    if records != OUTSTANDING || dup + foreign + rereport + wrong_seq != 0 {
        msw_fail("reply leg");
    }
}

fn percentile(samples: &mut [u64], pct: usize) -> u64 {
    samples.sort_unstable();
    let index = (samples.len() * pct / 100).min(samples.len().saturating_sub(1));
    samples.get(index).copied().unwrap_or(0)
}

/// M5: terminal→running latency, `WaitCompletion(IPC)` against `IpcWait`,
/// alternating rounds to the same provider in the same thread.
fn msw_m5_leg(provider: usize) {
    let mut completion_wait: Vec<u64> = Vec::with_capacity(M5_ROUNDS);
    let mut ipc_wait: Vec<u64> = Vec::with_capacity(M5_ROUNDS);
    let mut reply_buf = [0u8; IPC_BUF_SIZE];
    for round in 0..2 * M5_ROUNDS {
        let started = sys_get_time();
        let call = submit_or_fail(provider, &[REQ_TAG, (round % 200) as u8]);
        if round % 2 == 0 {
            match wait_any(IPC | TIMER, 200) {
                Ok(Wake::Ipc(op)) if op == call.operation() => {}
                _ => msw_fail("m5 completion wait"),
            }
            take_or_fail(&call, &mut reply_buf);
            completion_wait.push(sys_get_time().saturating_sub(started));
        } else {
            loop {
                if let Ok(Some(_)) = call.try_take(&mut reply_buf) {
                    break;
                }
                if ostd::syscall::sys_ipc_wait(200) != 1 {
                    msw_fail("m5 IpcWait");
                }
            }
            ipc_wait.push(sys_get_time().saturating_sub(started));
        }
    }
    let tick = ostd::syscall::sys_get_timer_freq().unwrap_or(0) / 100;
    let (wc50, wc99) = (
        percentile(&mut completion_wait, 50),
        percentile(&mut completion_wait, 99),
    );
    let (iw50, iw99) = (percentile(&mut ipc_wait, 50), percentile(&mut ipc_wait, 99));
    let pass = tick != 0 && wc99 <= iw99 + tick;
    println(&format!(
        "[async-lifecycle] MSW-M5 rounds={M5_ROUNDS} wc_p50={wc50} wc_p99={wc99} \
         ipcwait_p50={iw50} ipcwait_p99={iw99} tick={tick} pass={pass}"
    ));
    if !pass {
        msw_fail("M5 latency above IpcWait p99 + 1 tick");
    }
}

/// Mixed NET_RX + IPC + TIMER. x86 has no NET_RX producer (D-8): the NET_RX
/// bit can only show its raw-mailbox interruption rule here, never a record.
fn msw_mixed_leg(provider: usize, raw_source: usize) {
    let all = NET_RX | IPC | TIMER;
    let mut reply_buf = [0u8; IPC_BUF_SIZE];
    let mut raw = [0u8; IPC_BUF_SIZE];
    let raw_from_source = |raw: &mut [u8]| {
        matches!(sys_recv(raw_source, raw), SyscallResult::Ok(s) if s == raw_source)
            && raw[0] == RAW_TAG
    };
    // Every wait goes through this counter, so `netrx_records` is measured: on
    // x86 (no NET_RX producer) it must stay zero.
    let mut netrx_records = 0usize;
    let mut wait = |sources: u32, ticks: u64| {
        let wake = wait_any(sources, ticks);
        if matches!(wake, Ok(Wake::NetRx)) {
            netrx_records += 1;
        }
        wake
    };
    let timer = matches!(wait(all, 5), Ok(Wake::Timer));

    // A raw message interrupts a wait whose mask names NET_RX.
    let _ = sys_send(raw_source, &[CTRL_TAG]);
    let interrupted = matches!(wait(all, 500), Ok(Wake::Interrupted));
    let raw_ok = raw_from_source(&mut raw);
    let call = submit_or_fail(provider, &[REQ_TAG, 1]);
    let ipc_after_raw = matches!(wait(all, 500), Ok(Wake::Ipc(op)) if op == call.operation());
    take_or_fail(&call, &mut reply_buf);

    // Without NET_RX in the mask the same raw message does not end the wait,
    // and a pending IPC terminal is still reported past the queued message.
    let _ = sys_send(raw_source, &[CTRL_TAG]);
    let raw_ignored = matches!(wait(IPC | TIMER, 30), Ok(Wake::Timer));
    let call = submit_or_fail(provider, &[REQ_TAG, 2]);
    let ipc_second = matches!(wait(IPC | TIMER, 500), Ok(Wake::Ipc(op)) if op == call.operation());
    take_or_fail(&call, &mut reply_buf);
    let raw2_ok = raw_from_source(&mut raw);
    #[cfg(target_arch = "x86_64")]
    let unallocated_irq_refused = matches!(wait_any(IRQ, 5), Err(ostd::ipc::IpcError::Recv));
    #[cfg(not(target_arch = "x86_64"))]
    let unallocated_irq_refused = true;
    println(&format!(
        "[async-lifecycle] MSW-MIXED timer={timer} interrupted_by_raw={interrupted} \
         ipc_after_raw={ipc_after_raw} raw_ignored_without_netrx={raw_ignored} \
         ipc_second={ipc_second} netrx_records={netrx_records} netrx_producer=absent-x86 \
         unallocated_irq_refused={unallocated_irq_refused}"
    ));
    if !(timer
        && interrupted
        && raw_ok
        && ipc_after_raw
        && raw_ignored
        && raw2_ok
        && ipc_second
        && netrx_records == 0
        && unallocated_irq_refused)
    {
        msw_fail("mixed leg");
    }
}

/// Peer death: every outstanding call is reported once and takes `PeerGone`.
fn msw_peer_death_leg(provider: usize) {
    let calls: Vec<PendingCall> = (0..DEATH_OUTSTANDING)
        .filter_map(|seq| PendingCall::submit(provider, &[REQ_TAG, seq as u8]).ok())
        .collect();
    let mut seen = alloc::vec![false; calls.len()];
    let (mut records, mut dup) = (0usize, 0usize);
    let deadline = ticks() + 2_000;
    while records < calls.len() && ticks() < deadline {
        match wait_any(IPC | TIMER, 200) {
            Ok(Wake::Ipc(op)) => match calls.iter().position(|call| call.operation() == op) {
                Some(index) if seen[index] => dup += 1,
                Some(index) => {
                    seen[index] = true;
                    records += 1;
                }
                None => msw_fail("peer death foreign token"),
            },
            Ok(Wake::Timer) => {}
            _ => msw_fail("peer death wait"),
        }
    }
    let mut reply_buf = [0u8; IPC_BUF_SIZE];
    let peer_gone = calls
        .iter()
        .filter(|call| {
            take_or_fail(call, &mut reply_buf).terminal == ostd::ipc::IpcTerminal::PeerGone
        })
        .count();
    let unterminal = calls.len() - records;
    println(&format!(
        "[async-lifecycle] MSW-DEATH outstanding={} records={records} peer_gone={peer_gone} \
         dup={dup} unterminal={unterminal}",
        calls.len()
    ));
    if calls.is_empty() || unterminal != 0 || dup != 0 || peer_gone != calls.len() {
        msw_fail("peer death leg");
    }
}

/// Sustained two-producer load: B's results must be reported while A floods,
/// with at most `MAX_OVERTAKES` other records between B's reply and its record.
fn msw_starvation_leg(fast: usize, slow: usize) {
    struct Outstanding {
        call: PendingCall,
        slow: bool,
    }
    let mut outstanding: Vec<Outstanding> = Vec::new();
    let mut seq = 0u8;
    let mut submit = |outstanding: &mut Vec<Outstanding>, slow_peer: bool| {
        seq = seq.wrapping_add(1) % 0xF0;
        let peer = if slow_peer { slow } else { fast };
        outstanding.push(Outstanding {
            call: submit_or_fail(peer, &[REQ_TAG, seq]),
            slow: slow_peer,
        });
    };
    for _ in 0..A_OUTSTANDING {
        submit(&mut outstanding, false);
    }
    for _ in 0..B_OUTSTANDING {
        submit(&mut outstanding, true);
    }
    let (mut a_done, mut b_done, mut b_during_flood) = (0usize, 0usize, 0usize);
    let (mut a_sent, mut b_sent) = (A_OUTSTANDING, B_OUTSTANDING);
    let (mut dup, mut foreign) = (0usize, 0usize);
    let mut taken: Vec<usize> = Vec::new();
    let mut arrivals: Vec<u64> = Vec::new();
    let mut b_windows: Vec<(u64, u64)> = Vec::new();
    let mut reply_buf = [0u8; IPC_BUF_SIZE];
    let deadline = ticks() + 6_000;
    // A keeps flooding (at least A_TARGET calls) until every B call has been
    // reported, so each B result competes with a live A backlog.
    while !outstanding.is_empty() && ticks() < deadline {
        let op = match wait_any(IPC | TIMER, 200) {
            Ok(Wake::Ipc(op)) => op,
            Ok(Wake::Timer) => continue,
            _ => msw_fail("starvation wait"),
        };
        let arrived = sys_get_time();
        let Some(index) = outstanding.iter().position(|o| o.call.operation() == op) else {
            // Counted, not fatal: a second record for a taken token is a
            // duplicate, any other unknown token is foreign.
            if taken.contains(&op) {
                dup += 1;
            } else {
                foreign += 1;
            }
            continue;
        };
        arrivals.push(arrived);
        taken.push(op);
        let entry = outstanding.swap_remove(index);
        let completion = take_or_fail(&entry.call, &mut reply_buf);
        if !completion.is_definite() {
            msw_fail("starvation terminal");
        }
        if entry.slow {
            b_done += 1;
            if outstanding.iter().any(|o| !o.slow) {
                b_during_flood += 1;
            }
            let sent = u64::from_le_bytes(reply_buf[2..10].try_into().unwrap());
            b_windows.push((sent, arrived));
            if b_sent < B_TARGET {
                b_sent += 1;
                submit(&mut outstanding, true);
            }
        } else {
            a_done += 1;
            if a_sent < A_TARGET || b_done < B_TARGET {
                a_sent += 1;
                submit(&mut outstanding, false);
            }
        }
    }
    // Anything still outstanding when the watchdog ended the loop was never
    // reported: that is a lost result.
    let lost = outstanding.len();
    let max_overtakes = b_windows
        .iter()
        .map(|&(sent, arrived)| {
            arrivals
                .iter()
                .filter(|&&t| t > sent && t < arrived)
                .count()
        })
        .max()
        .unwrap_or(0);
    println(&format!(
        "[async-lifecycle] MSW-STARVATION a_done={a_done} b_done={b_done} \
         b_during_flood={b_during_flood} records={} max_b_overtakes={max_overtakes} \
         bound={MAX_OVERTAKES} lost={lost} dup={dup} foreign={foreign}",
        arrivals.len()
    ));
    if lost + dup + foreign != 0
        || a_done < A_TARGET
        || b_done != B_TARGET
        || b_during_flood != B_TARGET
        || max_overtakes > MAX_OVERTAKES
    {
        msw_fail("starvation leg");
    }
}

/// Deadlines (wait TIMER and real operation expiry) and a refused late reply.
fn msw_deadline_and_stale_leg(hold: usize) {
    let mut reply_buf = [0u8; IPC_BUF_SIZE];
    let submitted = ticks();
    let held = submit_or_fail(hold, &[REQ_TAG, 1]);
    let started = ticks();
    let timer_ok = matches!(wait_any(IPC | TIMER, 20), Ok(Wake::Timer));
    let timer_ticks = ticks() - started;
    let expired = match wait_any(IPC, 0) {
        Ok(Wake::Ipc(op)) => op == held.operation(),
        _ => false,
    };
    let expiry_ticks = ticks() - submitted;
    let terminal = take_or_fail(&held, &mut reply_buf).terminal;
    // Let the provider try its late reply, then prove only a fresh token answers.
    // Every record until the fresh one, and in the quiet window after it, is
    // classified: one naming the expired token is a stale record.
    let _ = sys_send(hold, &[CTRL_TAG, 0]);
    let fresh = submit_or_fail(hold, &[REQ_TAG, 2]);
    let mut stale_records = 0usize;
    let mut other_records = 0usize;
    let (mut fresh_ok, mut late_refused) = (false, false);
    let fresh_deadline = ticks() + 500;
    while !fresh_ok && ticks() < fresh_deadline {
        match wait_any(IPC | TIMER, 100) {
            Ok(Wake::Ipc(op)) if op == fresh.operation() => {
                take_or_fail(&fresh, &mut reply_buf);
                fresh_ok = true;
                late_refused = reply_buf[1] == 1;
            }
            Ok(Wake::Ipc(op)) if op == held.operation() => stale_records += 1,
            Ok(Wake::Ipc(_)) => other_records += 1,
            Ok(_) => {}
            Err(_) => msw_fail("stale leg wait"),
        }
    }
    let quiet = match wait_any(IPC | TIMER, 5) {
        Ok(Wake::Timer) => true,
        Ok(Wake::Ipc(op)) if op == held.operation() => {
            stale_records += 1;
            false
        }
        _ => false,
    };
    let _ = sys_send(hold, &[REQ_TAG, DONE_SEQ]);
    println(&format!(
        "[async-lifecycle] MSW-DEADLINE timer={timer_ok} timer_ticks={timer_ticks} \
         op_expiry={expired} op_expiry_ticks={expiry_ticks} terminal={terminal:?}"
    ));
    println(&format!(
        "[async-lifecycle] MSW-STALE late_reply_refused={late_refused} fresh_reply={fresh_ok} \
         stale_records={stale_records} other_records={other_records} quiet_after={quiet}"
    ));
    let timer_in_window = (20..=22).contains(&timer_ticks);
    if !(timer_ok
        && timer_in_window
        && expired
        && expiry_ticks >= OPERATION_TIMEOUT_TICKS
        && terminal == ostd::ipc::IpcTerminal::Indeterminate
        && fresh_ok
        && late_refused
        && quiet
        && stale_records == 0
        && other_records == 0)
    {
        msw_fail("deadline or stale leg");
    }
}

/// Orchestrator side: spawn the providers and the caller, hand the caller the
/// provider tids, and wait for its verdict or its exit.
fn multi_source_leg() {
    let echo = spawn(MSW_ECHO);
    let slow = spawn(MSW_SLOW);
    let hold = spawn(MSW_HOLD);
    let vanish = spawn(VANISH_ROLE);
    let echo_reverse = spawn(&format!("{ECHO_PREFIX}{OUTSTANDING}"));
    let raw = spawn(MSW_RAW);
    let caller = spawn(MSW_CALLER);
    settle();
    if !matches!(sys_notify_on_exit(caller), SyscallResult::Ok(0)) {
        fail("multi-source caller exit watch");
    }
    let mut start = [0u8; 1 + 8 * 6];
    start[0] = CTRL_TAG;
    for (index, tid) in [echo, slow, hold, vanish, echo_reverse, raw]
        .into_iter()
        .enumerate()
    {
        start[1 + 8 * index..9 + 8 * index].copy_from_slice(&(tid as u64).to_le_bytes());
    }
    if !matches!(sys_send(caller, &start), SyscallResult::Ok(_)) {
        fail("multi-source caller start");
    }
    let mut verdict = [0u8; 16];
    match sys_recv(caller, &mut verdict) {
        SyscallResult::Ok(sender) if sender == caller && verdict[..2] == [CTRL_TAG, 1] => {
            println("[async-lifecycle] MSW PASS");
        }
        _ => fail("multi-source caller did not report success"),
    }
}

// ── Phase 03e: dispatched cancellation, sync timeout, dropped interest ───────
//
// The orchestrator (`bench`) drives these through the SDK's `IpcWait` path.

const RACE_ROLE: &str = "c2c-race-provider";
const SYNC_HOLD_ROLE: &str = "c2c-sync-hold";
const DROP_ROLE: &str = "c2c-drop-provider";
/// Raw control steps between the orchestrator and the 03e providers.
const STEP_START: u8 = 0;
const STEP_DONE: u8 = 2;
const STEP_REPORT: u8 = 3;
const STEP_AWAKE: u8 = 4;
const STEP_LATE: u8 = 5;
/// Submit-then-cancel iterations of the cancellation race.
const RACE_CALLS: usize = 256;
/// The sync-hold provider sleeps through the first (queued) bounded call, then
/// holds the second (dispatched) one past its budget.
const SYNC_QUEUED_HOLD_TICKS: usize = 40;
const SYNC_DISPATCHED_HOLD_TICKS: usize = 60;
const SYNC_SHORT_BUDGET: u64 = 10;
const SYNC_BUDGET: u64 = 20;
/// Dropped-interest rounds; each round drops a full owner quota of handles.
const DROP_ROUNDS: usize = 3;
const DROP_BATCH: usize = 64;
/// Yields one admission may spend on `Busy` before the leg calls it a leak.
const ADMISSION_YIELDS: usize = 4_000;

fn start_provider(role: &str) -> usize {
    let provider = spawn(role);
    settle();
    if !matches!(
        sys_send(provider, &[CTRL_TAG, STEP_START]),
        SyscallResult::Ok(_)
    ) {
        fail("03e provider start");
    }
    provider
}

/// The orchestrator's tid, from the provider's raw start frame.
fn await_start(buf: &mut [u8]) -> usize {
    match sys_recv(0, buf) {
        SyscallResult::Ok(sender) if buf[..2] == [CTRL_TAG, STEP_START] => sender,
        _ => fail("03e provider start frame"),
    }
}

/// Masked provider for CANCEL-RACE: receives only from the orchestrator,
/// records every request it was handed and never replies, so a delivered
/// request can only end `Indeterminate`.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_race_provider() -> ! {
    let mut buf = [0u8; IPC_BUF_SIZE];
    let caller = await_start(&mut buf);
    let mut seen = [0u8; RACE_CALLS / 8];
    let mut twice = 0u8;
    loop {
        match sys_recv(caller, &mut buf) {
            SyscallResult::Ok(_) if buf[..2] == [CTRL_TAG, STEP_DONE] => break,
            SyscallResult::Ok(_) if buf[0] == REQ_TAG => {
                let seq = usize::from(u16::from_le_bytes([buf[1], buf[2]]));
                if seq >= RACE_CALLS {
                    fail("race provider sequence");
                }
                if seen[seq / 8] & (1 << (seq % 8)) != 0 {
                    twice = twice.saturating_add(1);
                }
                seen[seq / 8] |= 1 << (seq % 8);
            }
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(_) => fail("race provider recv"),
        }
    }
    // The DONE frame is raw, so this Send is raw too (no served operation).
    let mut report = [0u8; 3 + RACE_CALLS / 8];
    report[..3].copy_from_slice(&[CTRL_TAG, STEP_REPORT, twice]);
    report[3..].copy_from_slice(&seen);
    let _ = sys_send(caller, &report);
    sys_exit(0)
}

/// Dispatched cancellation is never reported as "not executed": a request the
/// provider received ends `Indeterminate`; `Cancelled` only for one it never saw.
fn cancel_race_leg() {
    use ostd::ipc::IpcTerminal;
    let provider = start_provider(RACE_ROLE);
    let mut outcome: Vec<Option<IpcTerminal>> = alloc::vec![None; RACE_CALLS];
    let mut reply_buf = [0u8; IPC_BUF_SIZE];
    for (seq, slot) in outcome.iter_mut().enumerate() {
        let [lo, hi] = (seq as u16).to_le_bytes();
        let call = admit(provider, &[REQ_TAG, lo, hi]);
        // Vary how far the provider gets before the cancel lands.
        for _ in 0..seq % 4 {
            yield_now();
        }
        if call.cancel().is_err() {
            fail("cancel race cancel");
        }
        match call.try_take(&mut reply_buf) {
            Ok(Some(completion)) => *slot = Some(completion.terminal),
            _ => fail("cancel race take"),
        }
    }
    if !matches!(
        sys_send(provider, &[CTRL_TAG, STEP_DONE]),
        SyscallResult::Ok(_)
    ) {
        fail("cancel race release");
    }
    let mut report = [0u8; 3 + RACE_CALLS / 8];
    if !recv_frame_from(provider, &mut report, STEP_REPORT) {
        fail("cancel race report");
    }
    let received = |seq: usize| report[3 + seq / 8] & (1 << (seq % 8)) != 0;
    let (mut cancelled, mut indeterminate, mut other, mut unterminal) = (0, 0, 0, 0);
    let (mut cancelled_but_received, mut received_but_definite) = (0, 0);
    for (seq, terminal) in outcome.iter().enumerate() {
        match terminal {
            Some(IpcTerminal::Cancelled) => cancelled += 1,
            Some(IpcTerminal::Indeterminate) => indeterminate += 1,
            Some(_) => other += 1,
            None => unterminal += 1,
        }
        let definite = matches!(
            terminal,
            Some(IpcTerminal::Cancelled | IpcTerminal::PreDispatchTimeout | IpcTerminal::Reply)
        );
        if received(seq) && definite {
            received_but_definite += 1;
        }
        if received(seq) && *terminal == Some(IpcTerminal::Cancelled) {
            cancelled_but_received += 1;
        }
    }
    let twice = report[2];
    println(&format!(
        "[async-lifecycle] CANCEL-RACE total={RACE_CALLS} cancelled={cancelled} \
         indeterminate={indeterminate} other={other} cancelled_but_received={cancelled_but_received} \
         received_but_definite={received_but_definite} delivered_twice={twice} unterminal={unterminal}"
    ));
    if cancelled + indeterminate != RACE_CALLS
        || cancelled_but_received != 0
        || received_but_definite != 0
        || twice != 0
        || unterminal != 0
    {
        fail("dispatched cancellation reported a delivered request as not executed");
    }
}

/// Submit, retrying only `Busy` (nothing was accepted) under a yield watchdog.
fn admit(provider: usize, request: &[u8]) -> PendingCall {
    for _ in 0..ADMISSION_YIELDS {
        match PendingCall::submit(provider, request) {
            Ok(call) => return call,
            Err(ostd::ipc::IpcSubmitError::Busy) => yield_now(),
            Err(_) => fail("03e admission"),
        }
    }
    fail("admission stayed Busy: a released slot was never returned")
}

/// Sync-timeout provider: sleep through the caller's queued call, then hold its
/// dispatched call past the budget and try both late replies.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_sync_hold() -> ! {
    let mut buf = [0u8; IPC_BUF_SIZE];
    let caller = await_start(&mut buf);
    let _ = ostd::syscall::sys_set_timer(SYNC_QUEUED_HOLD_TICKS);
    let _ = sys_send(caller, &[CTRL_TAG, STEP_AWAKE]);
    let first_seq = match sys_recv(caller, &mut buf) {
        SyscallResult::Ok(_) if buf[0] == REQ_TAG => buf[1],
        _ => fail("sync hold first request"),
    };
    let held = current().unwrap_or_else(|| fail("sync hold token"));
    let _ = ostd::syscall::sys_set_timer(SYNC_DISPATCHED_HOLD_TICKS);
    // Late implicit reply: Send to the still-served caller is the operation's
    // reply bridge. It must be quarantined, never reach the raw mailbox.
    let _ = sys_send(caller, &[REPLY_TAG, first_seq]);
    let explicit_refused = reply(held, &[REPLY_TAG, first_seq]).is_err();
    let _ = sys_send(
        caller,
        &[CTRL_TAG, STEP_LATE, first_seq, explicit_refused as u8],
    );
    loop {
        match sys_recv(caller, &mut buf) {
            SyscallResult::Ok(_) if buf[..2] == [CTRL_TAG, STEP_DONE] => sys_exit(0),
            SyscallResult::Ok(_) if buf[0] == REQ_TAG => {
                let operation = current().unwrap_or_else(|| fail("sync hold fresh token"));
                if reply(operation, &[REPLY_TAG, buf[1]]).is_err() {
                    fail("sync hold fresh reply");
                }
            }
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(_) => fail("sync hold recv"),
        }
    }
}

/// The synchronous SDK's caller budget: a queued expiry is `PreDispatchTimeout`
/// and is never delivered; a dispatched expiry is `Indeterminate`; both late
/// replies are refused; the next call returns only its own reply.
fn sync_timeout_late_leg() {
    use ostd::ipc::{service_call_bounded, IpcError};
    let provider = start_provider(SYNC_HOLD_ROLE);
    let (mut send, mut recv) = ([0u8; 32], [0u8; 32]);
    let queued = service_call_bounded(
        provider,
        &(REQ_TAG, 0u8),
        &mut send,
        &mut recv,
        SYNC_SHORT_BUDGET,
    )
    .err();
    let mut frame = [0u8; 8];
    if !recv_frame_from(provider, &mut frame, STEP_AWAKE) {
        fail("sync hold awake");
    }
    let dispatched =
        service_call_bounded(provider, &(REQ_TAG, 1u8), &mut send, &mut recv, SYNC_BUDGET).err();
    // The first frame from the provider after expiry must be its report: the
    // quarantined bridge reply never lands in the raw mailbox.
    let late_send_raw_absent = recv_frame_from(provider, &mut frame, STEP_LATE);
    let queued_delivered = late_send_raw_absent && frame[2] != 1;
    let late_explicit_refused = late_send_raw_absent && frame[3] == 1;
    let next = service_call_bounded(provider, &(REQ_TAG, 2u8), &mut send, &mut recv, 200)
        .map(|bytes| bytes == [REPLY_TAG, 2])
        .unwrap_or(false);
    let _ = sys_send(provider, &[CTRL_TAG, STEP_DONE]);
    println(&format!(
        "[async-lifecycle] SYNC-TIMEOUT-LATE queued={queued:?} queued_delivered={queued_delivered} \
         first={dispatched:?} late_explicit_refused={late_explicit_refused} \
         late_send_raw_absent={late_send_raw_absent} next_reply={next}"
    ));
    if queued != Some(IpcError::PreDispatchTimeout)
        || queued_delivered
        || dispatched != Some(IpcError::Indeterminate)
        || !late_explicit_refused
        || !late_send_raw_absent
        || !next
    {
        fail("sync timeout leg");
    }
}

/// Dropped-interest provider: replies at once to even sequences and holds odd
/// ones; on DONE tries a late reply to every held token.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_drop_provider() -> ! {
    let mut buf = [0u8; IPC_BUF_SIZE];
    let caller = await_start(&mut buf);
    let mut held: Vec<usize> = Vec::new();
    loop {
        match sys_recv(caller, &mut buf) {
            SyscallResult::Ok(_) if buf[..2] == [CTRL_TAG, STEP_DONE] => break,
            SyscallResult::Ok(_) if buf[0] == REQ_TAG => {
                let operation = current().unwrap_or_else(|| fail("drop provider token"));
                if buf[1] % 2 == 0 {
                    // The caller may already have dropped it; a refusal is fine.
                    let _ = reply(operation, &[REPLY_TAG, buf[1]]);
                } else {
                    held.push(operation);
                }
            }
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(_) => fail("drop provider recv"),
        }
    }
    let late_accepted = held
        .iter()
        .filter(|operation| reply(**operation, &[REPLY_TAG, 0xEE]).is_ok())
        .count();
    let _ = sys_send(
        caller,
        &[
            CTRL_TAG,
            STEP_REPORT,
            late_accepted.min(255) as u8,
            held.len().min(255) as u8,
        ],
    );
    sys_exit(0)
}

/// Dropping an unsettled `PendingCall` releases its kernel slot: three full
/// owner quotas of dropped handles, then a fourth quota is admitted.
fn drop_reclaim_leg() {
    let provider = start_provider(DROP_ROLE);
    let mut dropped = 0usize;
    for round in 0..=DROP_ROUNDS {
        let calls: Vec<PendingCall> = (0..DROP_BATCH)
            .map(|index| admit(provider, &[REQ_TAG, (round * DROP_BATCH + index) as u8]))
            .collect();
        // Let the provider answer some and hold others before the handles go.
        settle();
        if round < DROP_ROUNDS {
            dropped += calls.len();
        }
        drop(calls);
    }
    if !matches!(
        sys_send(provider, &[CTRL_TAG, STEP_DONE]),
        SyscallResult::Ok(_)
    ) {
        fail("drop provider release");
    }
    let mut report = [0u8; 8];
    if !recv_frame_from(provider, &mut report, STEP_REPORT) {
        fail("drop provider report");
    }
    let (late_accepted, held) = (report[2], report[3]);
    println(&format!(
        "[async-lifecycle] DROP-RECLAIM dropped={dropped} fresh_accepted={DROP_BATCH} \
         held_by_provider={held} late_accepted={late_accepted}"
    ));
    if dropped != DROP_ROUNDS * DROP_BATCH || late_accepted != 0 {
        fail("dropped interest left a reply slot or accepted a late reply");
    }
}

/// The next frame from `provider` is `[CTRL_TAG, step, ..]`. A masked `Recv`
/// may first return an unrelated queued death record (the MSW caller's exit
/// watch); those are skipped, anything else from `provider` is a mismatch.
fn recv_frame_from(provider: usize, frame: &mut [u8], step: u8) -> bool {
    for _ in 0..8 {
        match sys_recv(provider, frame) {
            SyscallResult::Ok(sender) if sender == provider => {
                return frame[..2] == [CTRL_TAG, step];
            }
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(_) => return false,
        }
    }
    false
}
