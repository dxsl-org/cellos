//! Local service-call lifecycle witness.
//!
//! Both synchronous SDK calls and asynchronous calls use kernel-owned bounded
//! operations. A provider that consumes a request and exits without replying
//! must terminate either caller with PeerGone, not leave a masked Recv stranded.
//! The same lane witnesses quota retention, real expiry and queued caller death.

use alloc::format;
use alloc::vec::Vec;
use api::c2c::RetryClass;
use api::ipc::IPC_BUF_SIZE;
use api::services::ipc::{VfsRequest, VfsResponse};
use api::task::TaskPriority;
use ostd::{
    cluster_endpoint::{CellMethod, EndpointError, LocalEndpoint},
    io::println,
    ipc::{submit, take, wait, IpcTakeResult, IpcTerminal},
    service::ServiceRef,
    syscall::{
        sys_exit, sys_lookup_service, sys_lookup_service_bound, sys_recv, sys_send,
        sys_set_spawn_args, sys_spawn_pinned, SyscallResult,
    },
    task::yield_now,
};

const PROBE_PATH: &str = "/bin/bench-probe";
const PROVIDER_ROLE: &str = "c2c-provider";
const SYNC_CALLER_PREFIX: &str = "c2c-sync-caller:";

/// First byte of the provider's request. Matches the postcard encoding of `(u8,)`.
const REQ_TAG: u8 = 0x5A;
/// Yields allowed for a spawned probe to reach its parked receive.
const SETTLE_YIELDS: usize = 60;
/// Per-call reply budget; 10 ms ticks, so 200 ticks ≈ 2 s per iteration.
const ASYNC_WAIT_TICKS: u64 = 200;
const ASYNC_WAIT_ROUNDS: usize = 10;

const CONTROL_TAG: u8 = 0xC0;
const QUOTA: usize = 64;
const DEAD_CALLS: usize = 4;
const DOOMED_CALLER_PREFIX: &str = "c2c-doomed-caller:";
/// All raw handshakes have a five-second watchdog.
const HANDSHAKE_TICKS: u64 = 500;
/// The real kernel deadline is 3000 scheduler ticks; never override it here.
const DEADLINE_TICKS: u64 = 3_000;
/// Keep the deadline provider out of Recv until both real deadlines expire.
const DEADLINE_HOLD_TICKS: usize = 3_200;

fn scheduler_ticks() -> u64 {
    ostd::syscall::sys_get_scheduler_ticks()
        .unwrap_or_else(|| fail("scheduler clock unavailable"))
}

/// TrySend is deliberately raw: it must not settle a captured operation.
fn raw_send(peer: usize, bytes: &[u8]) {
    let start = scheduler_ticks();
    loop {
        match ostd::syscall::sys_try_send(peer, bytes) {
            SyscallResult::Ok(0) => return,
            SyscallResult::Ok(usize::MAX) => {}
            _ => fail("raw handshake send"),
        }
        if scheduler_ticks().saturating_sub(start) >= HANDSHAKE_TICKS {
            fail("raw handshake send watchdog");
        }
        yield_now();
    }
}

fn bounded_recv(mask: usize, bytes: &mut [u8]) -> usize {
    match ostd::syscall::sys_recv_timeout(mask, bytes, HANDSHAKE_TICKS) {
        SyscallResult::Ok(sender) if sender != 0 => sender,
        _ => fail("receive watchdog"),
    }
}

fn control_recv(peer: usize, step: u8) {
    let mut bytes = [0u8; 32];
    if bounded_recv(peer, &mut bytes) != peer || bytes[..2] != [CONTROL_TAG, step] {
        fail("raw control handshake");
    }
}

fn mailbox_empty() {
    let mut bytes = [0u8; 32];
    if !matches!(ostd::syscall::sys_try_recv(0, &mut bytes), SyscallResult::Ok(0)) {
        fail("excluded request was delivered");
    }
}

fn expect_busy(peer: usize, sequence: u8) {
    if !matches!(
        ostd::ipc::PendingCall::submit(peer, &[REQ_TAG, sequence]),
        Err(ostd::ipc::IpcSubmitError::Busy)
    ) {
        fail("quota did not return Busy");
    }
}

fn take_reply(call: &ostd::ipc::PendingCall, sequence: u8) {
    let mut bytes = [0u8; 32];
    let completion = call.wait_and_take(&mut bytes, ASYNC_WAIT_TICKS, ASYNC_WAIT_ROUNDS)
        .unwrap_or_else(|_| fail("reply take"))
        .unwrap_or_else(|| fail("reply take watchdog"));
    if completion.terminal != IpcTerminal::Reply
        || completion.len != 2 || bytes[..2] != [REQ_TAG, sequence]
    {
        fail("reply identity or terminal");
    }
    if call.try_take(&mut bytes) != Err(ostd::ipc::IpcError::InvalidOperation) {
        fail("operation was taken more than once");
    }
}

fn fail(reason: &str) -> ! {
    println(&format!("[local-lifecycle] FAIL — {reason}"));
    sys_exit(1)
}

/// Spawn `/bin/bench-probe` with one role argument.
fn spawn_probe(role: &str) -> Result<usize, ()> {
    if !sys_set_spawn_args(role) {
        return Err(());
    }
    match sys_spawn_pinned(PROBE_PATH, TaskPriority::Normal as u8, 0) {
        SyscallResult::Ok(tid) if tid != 0 => Ok(tid),
        _ => Err(()),
    }
}

// ── bench-probe roles ─────────────────────────────────────────────────────────

/// Provider role: consume exactly one tagged request, then die **without**
/// replying, so the caller's reply wait is the only thing left outstanding.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_provider() -> ! {
    println("[local-lifecycle] provider-ready");
    let mut buf = [0u8; 32];
    loop {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(_sender) if buf[0] == REQ_TAG => {
                println("[local-lifecycle] provider-exiting-without-reply");
                sys_exit(0);
            }
            _ => yield_now(),
        }
    }
}

/// Synchronous caller role: exercise the real SDK call path against a provider
/// that has already been told to die. Reaching the print after the call means
/// the kernel produced a bounded terminal for this shape.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_sync_caller(role: &str) -> ! {
    let Some(provider_tid) = role
        .strip_prefix(SYNC_CALLER_PREFIX)
        .and_then(|tid| tid.parse::<usize>().ok())
    else {
        println("[local-lifecycle] FAIL — sync caller argument");
        sys_exit(1)
    };
    println(&format!(
        "[local-lifecycle] sync-caller-start provider={provider_tid}"
    ));
    let mut send_buf = [0u8; IPC_BUF_SIZE];
    let mut recv_buf = [0u8; IPC_BUF_SIZE];
    let request = (REQ_TAG,);
    let result = ostd::ipc::service_call_typed::<(u8,), ()>(
        provider_tid,
        &request,
        &mut send_buf,
        &mut recv_buf,
    );
    if result != Err(ostd::ipc::IpcError::PeerGone) {
        fail("sync caller did not report PeerGone");
    }
    println("[local-lifecycle] SYNC-RESULT=RETURNED error=PeerGone");
    sys_exit(0)
}

/// Preserve the outer request context across a nested RPC, reply twice to prove
/// duplicate quarantine, then exit before the client consumes the retained reply.
#[allow(dead_code)]
pub fn run_nested_provider() -> ! {
    let mut request = [0u8; 32];
    let sender = match sys_recv(0, &mut request) {
        SyscallResult::Ok(sender) if request[0] == REQ_TAG => sender,
        _ => fail("nested provider request"),
    };
    let operation = ostd::ipc::current().unwrap_or_else(|| fail("nested provider token"));
    let mut vfs = ServiceRef::<{ api::syscall::service::VFS }>::new();
    let mut response = [0u8; IPC_BUF_SIZE];
    if !matches!(
        vfs.call::<VfsRequest, VfsResponse>(&VfsRequest::Stat("/"), &mut response),
        Ok(VfsResponse::Stat { .. })
    ) {
        fail("nested VFS RPC");
    }
    if ostd::ipc::current() != Some(operation) {
        fail("nested RPC replaced the inbound operation");
    }
    if !matches!(sys_send(sender, &[REQ_TAG, 7]), SyscallResult::Ok(0)) {
        fail("nested provider reply");
    }
    if !matches!(sys_send(sender, &[REQ_TAG, 8]), SyscallResult::Ok(0)) {
        fail("nested provider duplicate quarantine");
    }
    println("[local-lifecycle] NESTED-PROVIDER=REPLIED");
    sys_exit(0)
}

fn nested_reply_leg() {
    let provider = spawn_probe("c2c-nested-reply")
        .unwrap_or_else(|_| fail("nested provider spawn"));
    settle();
    let call = ostd::ipc::PendingCall::submit(provider, &[REQ_TAG])
        .unwrap_or_else(|_| fail("nested call submit"));
    for _ in 0..ASYNC_WAIT_ROUNDS {
        if ostd::ipc::wait(ASYNC_WAIT_TICKS) {
            break;
        }
    }
    settle();
    let mut short = [0u8; 1];
    if call.try_take(&mut short) != Err(ostd::ipc::IpcError::BufferTooSmall) {
        fail("short take consumed or lost retained reply");
    }
    let mut full = [0u8; 32];
    let completion = call.try_take(&mut full)
        .unwrap_or_else(|_| fail("retained reply take"))
        .unwrap_or_else(|| fail("retained reply missing"));
    if completion.terminal != IpcTerminal::Reply || completion.len != 2 || full[..2] != [REQ_TAG, 7] {
        fail("reply was replaced by duplicate or peer death");
    }
    if call.try_take(&mut full) != Err(ostd::ipc::IpcError::InvalidOperation) {
        fail("terminal taken twice");
    }
    if !matches!(ostd::syscall::sys_try_recv(provider, &mut full), SyscallResult::Ok(0)) {
        fail("duplicate reply escaped into ordinary mailbox");
    }
    println("[local-lifecycle] NESTED-REPLY=OK retained=true duplicate_quarantined=true");
}

#[allow(dead_code)]
pub fn run_reply_loop() -> ! {
    let mut request = [0u8; 32];
    for _ in 0..71 {
        let sender = match sys_recv(0, &mut request) {
            SyscallResult::Ok(sender) if request[0] == REQ_TAG => sender,
            _ => fail("reply loop request"),
        };
        if !matches!(sys_send(sender, &[REQ_TAG, 7]), SyscallResult::Ok(0)) {
            fail("reply loop response");
        }
    }
    sys_exit(0)
}

fn sdk_slot_cleanup_leg() {
    let provider = spawn_probe("c2c-reply-loop")
        .unwrap_or_else(|_| fail("reply loop spawn"));
    settle();
    let mut send = [0u8; 32];
    let mut short = [0u8; 1];
    // More than the kernel's per-owner operation capacity: an oversized-reply
    // error must drain its terminal rather than leak one slot on each call.
    for _ in 0..70 {
        if ostd::ipc::service_call(provider, &(REQ_TAG,), &mut send, &mut short)
            != Err(ostd::ipc::IpcError::BufferTooSmall)
        {
            fail("SDK oversized reply cleanup");
        }
    }
    let mut full = [0u8; 32];
    if ostd::ipc::service_call(provider, &(REQ_TAG,), &mut send, &mut full)
        != Ok(&[REQ_TAG, 7][..])
    {
        fail("SDK slot reuse or exact reply length");
    }
    println("[local-lifecycle] SDK-SLOT-CLEANUP=OK errors=70 reply_len=2");
}

#[allow(dead_code)]
pub fn run_cancel_provider() -> ! {
    let mut request = [0u8; 32];
    let sender = match sys_recv(0, &mut request) {
        SyscallResult::Ok(sender) => sender,
        _ => fail("cancel provider first receive"),
    };
    let first = ostd::ipc::current().unwrap_or_else(|| fail("first operation missing"));
    // Raw TrySend is an out-of-band handshake, not an operation reply. The
    // caller waits in masked Recv so cancellation is definitely post-dispatch.
    loop {
        if matches!(ostd::syscall::sys_try_send(sender, &[REQ_TAG, 1]), SyscallResult::Ok(0)) {
            break;
        }
        yield_now();
    }
    if !matches!(sys_recv(0, &mut request), SyscallResult::Ok(from) if from == sender) {
        fail("cancel provider second receive");
    }
    let second = ostd::ipc::current().unwrap_or_else(|| fail("second operation missing"));
    if first == second || ostd::ipc::reply(first, &[REQ_TAG, 1]).is_ok() {
        fail("cancelled operation accepted a late reply");
    }
    if ostd::ipc::reply(second, &[REQ_TAG, 2]).is_err() {
        fail("new operation reply");
    }
    sys_exit(0)
}

fn cancelled_reply_leg() {
    let provider = spawn_probe("c2c-cancel-reply")
        .unwrap_or_else(|_| fail("cancel provider spawn"));
    settle();
    let first = ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, 1])
        .unwrap_or_else(|_| fail("cancel first submit"));
    let mut response = [0u8; 32];
    if !matches!(sys_recv(provider, &mut response), SyscallResult::Ok(from) if from == provider)
        || response[..2] != [REQ_TAG, 1]
    {
        fail("post-dispatch handshake");
    }
    first.cancel().unwrap_or_else(|_| fail("cancel first operation"));
    let terminal = first.try_take(&mut response)
        .unwrap_or_else(|_| fail("cancel terminal take"))
        .unwrap_or_else(|| fail("cancel terminal absent"));
    if terminal.terminal != IpcTerminal::Indeterminate {
        fail("post-dispatch cancellation claimed no execution");
    }
    let second = ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, 2])
        .unwrap_or_else(|_| fail("second submit"));
    let terminal = second.wait_and_take(&mut response, ASYNC_WAIT_TICKS, ASYNC_WAIT_ROUNDS)
        .unwrap_or_else(|_| fail("second take"))
        .unwrap_or_else(|| fail("second completion absent"));
    if terminal.terminal != IpcTerminal::Reply || terminal.len != 2 || response[..2] != [REQ_TAG, 2] {
        fail("late old reply settled new operation");
    }
    println("[local-lifecycle] CANCEL-LATE-REPLY=OK outcome=Indeterminate next_seq=2");
}

/// Receive each operation before the next submit, bypassing the peer's 16-wire
/// queue without releasing any of the caller's 64 completion reservations.
#[allow(dead_code)]
pub fn run_saturation_provider() -> ! {
    let mut operations = [0usize; QUOTA];
    let mut bytes = [0u8; 32];
    let mut caller = 0;
    for (sequence, operation) in operations.iter_mut().enumerate() {
        let sender = bounded_recv(0, &mut bytes);
        if bytes[..2] != [REQ_TAG, sequence as u8] || (caller != 0 && caller != sender) {
            fail("saturation provider request sequence");
        }
        caller = sender;
        *operation = ostd::ipc::current()
            .unwrap_or_else(|| fail("saturation provider token"));
        raw_send(caller, &[CONTROL_TAG, sequence as u8]);
    }
    // A Busy request would be ahead of this raw command in the same mailbox.
    if bounded_recv(0, &mut bytes) != caller || bytes[..2] != [CONTROL_TAG, 64] {
        fail("quota Busy request reached provider");
    }
    mailbox_empty();
    for (sequence, operation) in operations.iter().enumerate() {
        if ostd::ipc::reply(*operation, &[REQ_TAG, sequence as u8]).is_err() {
            fail("saturation provider exact reply");
        }
    }
    raw_send(caller, &[CONTROL_TAG, 64]);
    if bounded_recv(0, &mut bytes) != caller || bytes[..2] != [CONTROL_TAG, 65] {
        fail("retained-slot Busy request reached provider");
    }
    mailbox_empty();
    raw_send(caller, &[CONTROL_TAG, 65]);
    if bounded_recv(0, &mut bytes) != caller || bytes[..2] != [REQ_TAG, 64] {
        fail("saturation reuse request");
    }
    let operation = ostd::ipc::current().unwrap_or_else(|| fail("reuse provider token"));
    if ostd::ipc::reply(operation, &[REQ_TAG, 64]).is_err() {
        fail("saturation reuse reply");
    }
    println("[local-lifecycle] SATURATION-PROVIDER=OK received=65 busy_delivered=0");
    raw_send(caller, &[CONTROL_TAG, 66]);
    sys_exit(0)
}

fn saturation_leg() {
    let provider = spawn_probe("c2c-saturation")
        .unwrap_or_else(|_| fail("saturation provider spawn"));
    let mut calls = Vec::with_capacity(QUOTA);
    for sequence in 0..QUOTA {
        let call = ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, sequence as u8])
            .unwrap_or_else(|_| fail("full64 saturation submit"));
        control_recv(provider, sequence as u8);
        calls.push(call);
    }
    expect_busy(provider, 254);
    raw_send(provider, &[CONTROL_TAG, 64]);
    control_recv(provider, 64);
    // The provider has explicitly replied to all 64, but not one was taken.
    expect_busy(provider, 255);
    raw_send(provider, &[CONTROL_TAG, 65]);
    control_recv(provider, 65);
    for (sequence, call) in calls.iter().enumerate() {
        take_reply(call, sequence as u8);
    }
    let reused = ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, 64])
        .unwrap_or_else(|_| fail("saturation slot reuse"));
    take_reply(&reused, 64);
    control_recv(provider, 66);
    println("[local-lifecycle] SATURATION=OK accepted=64 busy_delivered=0 terminal_charged=64 drained=64 reuse=Reply");
}

/// Capture one dispatched token, then park on TIMER rather than Recv: the
/// second operation is genuinely queued for the entire real deadline window.
#[allow(dead_code)]
pub fn run_deadline_provider() -> ! {
    let mut bytes = [0u8; 32];
    let caller = bounded_recv(0, &mut bytes);
    if bytes[..2] != [REQ_TAG, 1] {
        fail("deadline dispatched request");
    }
    let operation = ostd::ipc::current()
        .unwrap_or_else(|| fail("deadline dispatched token"));
    raw_send(caller, &[CONTROL_TAG, 1]);
    if !matches!(
        ostd::syscall::sys_set_timer(DEADLINE_HOLD_TICKS),
        SyscallResult::Ok(_)
    ) {
        fail("deadline provider timer");
    }
    // Both operations must have expired before the caller releases this gate.
    if bounded_recv(0, &mut bytes) != caller || bytes[..2] != [CONTROL_TAG, 2] {
        fail("expired queued request reached provider");
    }
    mailbox_empty();
    if ostd::ipc::reply(operation, &[REQ_TAG, 1]).is_ok() {
        fail("expired dispatched operation accepted late reply");
    }
    raw_send(caller, &[CONTROL_TAG, 2]);
    let sender = bounded_recv(0, &mut bytes);
    if sender != caller || bytes[..2] != [REQ_TAG, 3] {
        fail("deadline fresh request");
    }
    let fresh = ostd::ipc::current().unwrap_or_else(|| fail("deadline fresh token"));
    if fresh == operation || ostd::ipc::reply(fresh, &[REQ_TAG, 3]).is_err() {
        fail("deadline fresh reply");
    }
    println("[local-lifecycle] DEADLINE-PROVIDER=OK queued_delivered=0 late_reply=refused");
    raw_send(caller, &[CONTROL_TAG, 3]);
    sys_exit(0)
}

fn deadline_leg() {
    let provider = spawn_probe("c2c-deadline")
        .unwrap_or_else(|_| fail("deadline provider spawn"));
    let dispatched_at = scheduler_ticks();
    let dispatched = ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, 1])
        .unwrap_or_else(|_| fail("deadline dispatched submit"));
    control_recv(provider, 1);
    let queued_at = scheduler_ticks();
    if queued_at.saturating_sub(dispatched_at) >= 100 {
        fail("deadline setup exceeded deterministic gate margin");
    }
    let queued = ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, 2])
        .unwrap_or_else(|_| fail("deadline queued submit"));
    if scheduler_ticks().saturating_sub(dispatched_at) >= 100 {
        fail("deadline queued admission exceeded gate margin");
    }
    let mut bytes = [0u8; 32];
    let mut dispatched_done = false;
    let mut queued_done = false;
    // Actual scheduler expiry, not cancellation or a fixture timeout override.
    for _ in 0..21 {
        for (call, start, expected, done) in [
            (&dispatched, dispatched_at, IpcTerminal::Indeterminate, &mut dispatched_done),
            (&queued, queued_at, IpcTerminal::PreDispatchTimeout, &mut queued_done),
        ] {
            if *done {
                continue;
            }
            if let Some(completion) = call.try_take(&mut bytes)
                .unwrap_or_else(|_| fail("deadline terminal take"))
            {
                if completion.terminal != expected || completion.len != 0
                    || scheduler_ticks().saturating_sub(start) < DEADLINE_TICKS
                {
                    fail("real deadline kind or elapsed ticks");
                }
                if call.try_take(&mut bytes) != Err(ostd::ipc::IpcError::InvalidOperation) {
                    fail("deadline terminal taken twice");
                }
                *done = true;
            }
        }
        if dispatched_done && queued_done {
            break;
        }
        let _ = wait(ASYNC_WAIT_TICKS);
    }
    if !dispatched_done || !queued_done {
        fail("real deadline watchdog");
    }
    raw_send(provider, &[CONTROL_TAG, 2]);
    control_recv(provider, 2);
    let fresh = ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, 3])
        .unwrap_or_else(|_| fail("deadline fresh submit"));
    take_reply(&fresh, 3);
    control_recv(provider, 3);
    println("[local-lifecycle] DEADLINE=OK queued=PreDispatchTimeout dispatched=Indeterminate elapsed_ticks>=3000 queued_delivered=0 reuse=Reply");
}

/// Mask the parent's raw gate while a different child queues work. The parent
/// opens the gate only after observing that child's real Exit notification.
#[allow(dead_code)]
pub fn run_caller_death_provider() -> ! {
    let mut bytes = [0u8; 32];
    let parent = bounded_recv(0, &mut bytes);
    if bytes[..2] != [CONTROL_TAG, 0] {
        fail("caller-death provider setup");
    }
    raw_send(parent, &[CONTROL_TAG, 0]);
    control_recv(parent, 1);
    mailbox_empty();
    raw_send(parent, &[CONTROL_TAG, 1]);
    if bounded_recv(0, &mut bytes) != parent || bytes[..2] != [REQ_TAG, 4] {
        fail("dead caller wire reached provider");
    }
    let operation = ostd::ipc::current()
        .unwrap_or_else(|| fail("caller-death fresh token"));
    if ostd::ipc::reply(operation, &[REQ_TAG, 4]).is_err() {
        fail("caller-death fresh reply");
    }
    println("[local-lifecycle] CALLER-DEATH-PROVIDER=OK dead_delivered=0 fresh=Reply");
    raw_send(parent, &[CONTROL_TAG, 2]);
    sys_exit(0)
}

#[allow(dead_code)]
pub fn run_doomed_caller(role: &str) -> ! {
    let provider = role.strip_prefix(DOOMED_CALLER_PREFIX)
        .and_then(|tid| tid.parse::<usize>().ok())
        .unwrap_or_else(|| fail("doomed caller argument"));
    let mut bytes = [0u8; 32];
    let parent = bounded_recv(0, &mut bytes);
    if bytes[..2] != [CONTROL_TAG, 0] {
        fail("doomed caller setup");
    }
    let mut calls = Vec::with_capacity(DEAD_CALLS);
    for sequence in 0..DEAD_CALLS {
        calls.push(ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, sequence as u8])
            .unwrap_or_else(|_| fail("doomed caller queued submit")));
    }
    raw_send(parent, &[CONTROL_TAG, DEAD_CALLS as u8]);
    control_recv(parent, 1);
    println("[local-lifecycle] CALLER-DEATH-CHILD=EXITING queued=4");
    sys_exit(0)
}

fn caller_death_leg() {
    let provider = spawn_probe("c2c-caller-death")
        .unwrap_or_else(|_| fail("caller-death provider spawn"));
    raw_send(provider, &[CONTROL_TAG, 0]);
    control_recv(provider, 0);
    let child = spawn_probe(&format!("{DOOMED_CALLER_PREFIX}{provider}"))
        .unwrap_or_else(|_| fail("doomed caller spawn"));
    raw_send(child, &[CONTROL_TAG, 0]);
    control_recv(child, DEAD_CALLS as u8);
    if !matches!(ostd::syscall::sys_notify_on_exit(child), SyscallResult::Ok(0)) {
        fail("caller-death exit watch");
    }
    raw_send(child, &[CONTROL_TAG, 1]);
    let mut exit = [0u8; 8];
    if bounded_recv(child, &mut exit) != child {
        fail("caller-death exit notification");
    }
    raw_send(provider, &[CONTROL_TAG, 1]);
    control_recv(provider, 1);
    let fresh = ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, 4])
        .unwrap_or_else(|_| fail("caller-death fresh submit"));
    take_reply(&fresh, 4);
    control_recv(provider, 2);
    println("[local-lifecycle] CALLER-DEATH=OK queued=4 exit_observed=true dead_delivered=0 fresh=Reply");
}

/// Exit only after the caller has installed its watch and released the gate.
#[allow(dead_code)]
pub fn run_restart_old() -> ! {
    let mut bytes = [0u8; 32];
    let caller = bounded_recv(0, &mut bytes);
    if bytes[..2] != [REQ_TAG, 5] || ostd::ipc::current().is_none() {
        fail("restart old dispatch");
    }
    raw_send(caller, &[CONTROL_TAG, 0]);
    control_recv(caller, 1);
    sys_exit(0)
}

/// A replacement is a new Cell incarnation, not authority over old tokens.
#[allow(dead_code)]
pub fn run_restart_new() -> ! {
    let mut bytes = [0u8; 32];
    let caller = bounded_recv(0, &mut bytes);
    if bytes[..2] != [REQ_TAG, 6] {
        fail("replacement request");
    }
    let old = usize::from_le_bytes(bytes[2..2 + core::mem::size_of::<usize>()]
        .try_into().unwrap());
    let fresh = ostd::ipc::current().unwrap_or_else(|| fail("replacement token"));
    if old == fresh || ostd::ipc::reply(old, &[REQ_TAG, 5]).is_ok() {
        fail("replacement accepted old token");
    }
    if ostd::ipc::reply(fresh, &[REQ_TAG, 6]).is_err() {
        fail("replacement fresh reply");
    }
    raw_send(caller, &[CONTROL_TAG, 2]);
    sys_exit(0)
}

fn restart_leg() {
    let old_provider = spawn_probe("c2c-restart-old")
        .unwrap_or_else(|_| fail("restart old spawn"));
    let old = ostd::ipc::PendingCall::submit(old_provider, &[REQ_TAG, 5])
        .unwrap_or_else(|_| fail("restart old submit"));
    control_recv(old_provider, 0);
    if !matches!(ostd::syscall::sys_notify_on_exit(old_provider), SyscallResult::Ok(0)) {
        fail("restart exit watch");
    }
    raw_send(old_provider, &[CONTROL_TAG, 1]);
    let mut bytes = [0u8; 32];
    if bounded_recv(old_provider, &mut bytes) != old_provider {
        fail("restart exit notification");
    }
    // Keep the old terminal reservation until after the replacement replies.
    let replacement = spawn_probe("c2c-restart-new")
        .unwrap_or_else(|_| fail("restart replacement spawn"));
    if replacement == old_provider {
        fail("replacement reused old task identity");
    }
    let mut request = [0u8; 2 + core::mem::size_of::<usize>()];
    request[..2].copy_from_slice(&[REQ_TAG, 6]);
    request[2..].copy_from_slice(&old.operation().to_le_bytes());
    let fresh = ostd::ipc::PendingCall::submit(replacement, &request)
        .unwrap_or_else(|_| fail("replacement submit"));
    take_reply(&fresh, 6);
    control_recv(replacement, 2);
    let completion = old.try_take(&mut bytes)
        .unwrap_or_else(|_| fail("restart old terminal"))
        .unwrap_or_else(|| fail("restart old terminal absent"));
    if completion.terminal != IpcTerminal::PeerGone || completion.len != 0
        || old.try_take(&mut bytes) != Err(ostd::ipc::IpcError::InvalidOperation)
    {
        fail("replacement altered old terminal");
    }
    if !matches!(ostd::ipc::PendingCall::submit(old_provider, &[REQ_TAG, 7]),
        Err(ostd::ipc::IpcSubmitError::PeerGone)) {
        fail("restart dead endpoint accepted work");
    }
    println("[local-lifecycle] RESTART=OK old=PeerGone replacement=Reply old_token=refused dead_submit=PeerGone");
}

/// Deliberately hold the request while the caller receives a third-Cell event.
#[allow(dead_code)]
pub fn run_event_provider() -> ! {
    let mut bytes = [0u8; 32];
    let caller = bounded_recv(0, &mut bytes);
    if bytes[..2] != [REQ_TAG, 8] {
        fail("event coexistence request");
    }
    let operation = ostd::ipc::current().unwrap_or_else(|| fail("event provider token"));
    raw_send(caller, &[CONTROL_TAG, 0]);
    control_recv(caller, 1);
    if ostd::ipc::reply(operation, &[REQ_TAG, 8]).is_err() {
        fail("event provider explicit reply");
    }
    raw_send(caller, &[CONTROL_TAG, 2]);
    sys_exit(0)
}

#[allow(dead_code)]
pub fn run_raw_event_source() -> ! {
    let mut bytes = [0u8; 32];
    let caller = bounded_recv(0, &mut bytes);
    if bytes[..2] != [CONTROL_TAG, 0] {
        fail("raw event source gate");
    }
    raw_send(caller, &[0xE1, 0xA7]);
    sys_exit(0)
}

fn event_coexistence_leg() {
    let provider = spawn_probe("c2c-event-provider")
        .unwrap_or_else(|_| fail("event provider spawn"));
    let call = ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, 8])
        .unwrap_or_else(|_| fail("event coexistence submit"));
    control_recv(provider, 0);
    let mut bytes = [0u8; 32];
    if call.try_take(&mut bytes) != Ok(None) {
        fail("RPC not pending before event");
    }
    let source = spawn_probe("c2c-raw-event")
        .unwrap_or_else(|_| fail("raw event source spawn"));
    raw_send(source, &[CONTROL_TAG, 0]);
    if bounded_recv(source, &mut bytes) != source || bytes[..2] != [0xE1, 0xA7] {
        fail("raw event payload or sender");
    }
    if call.try_take(&mut bytes) != Ok(None) {
        fail("raw event settled pending RPC");
    }
    raw_send(provider, &[CONTROL_TAG, 1]);
    take_reply(&call, 8);
    control_recv(provider, 2);
    if !matches!(ostd::syscall::sys_try_recv(provider, &mut bytes), SyscallResult::Ok(0)) {
        fail("operation reply leaked into raw mailbox");
    }
    println("[local-lifecycle] EVENT-COEXISTENCE=OK raw_event=received rpc_pending=true reply=correlated raw_reply_absent=true");
}

/// Accept A's full owner quota without replying, then serve independent caller B.
#[allow(dead_code)]
pub fn run_multi_caller_provider() -> ! {
    let mut bytes = [0u8; 32];
    let coordinator = bounded_recv(0, &mut bytes);
    if bytes[..2] != [CONTROL_TAG, 0] {
        fail("multi-caller provider setup");
    }
    raw_send(coordinator, &[CONTROL_TAG, 0]);
    let mut caller_a = 0;
    let mut operations = [0usize; QUOTA];
    for (sequence, operation) in operations.iter_mut().enumerate() {
        let sender = bounded_recv(0, &mut bytes);
        if sender == coordinator || bytes[..2] != [REQ_TAG, sequence as u8]
            || (caller_a != 0 && sender != caller_a) {
            fail("multi-caller A request identity");
        }
        caller_a = sender;
        *operation = ostd::ipc::current()
            .unwrap_or_else(|| fail("multi-caller A token"));
        raw_send(caller_a, &[CONTROL_TAG, sequence as u8]);
    }
    // A's overflow would precede B or remain visible in the same mailbox.
    if bounded_recv(0, &mut bytes) != coordinator || bytes[..2] != [REQ_TAG, 64] {
        fail("multi-caller B or rejected A overflow");
    }
    let call_b = ostd::ipc::current().unwrap_or_else(|| fail("multi-caller B token"));
    mailbox_empty();
    if operations.contains(&call_b) || ostd::ipc::reply(call_b, &[REQ_TAG, 64]).is_err() {
        fail("multi-caller B reply identity");
    }
    raw_send(coordinator, &[CONTROL_TAG, 2]);
    control_recv(coordinator, 3);
    for (sequence, operation) in operations.iter().enumerate() {
        if ostd::ipc::reply(*operation, &[REQ_TAG, sequence as u8]).is_err() {
            fail("multi-caller A exact reply");
        }
    }
    raw_send(caller_a, &[CONTROL_TAG, 64]);
    if bounded_recv(0, &mut bytes) != coordinator || bytes[..2] != [REQ_TAG, 65] {
        fail("multi-caller B second request or retained overflow delivery");
    }
    let second_b = ostd::ipc::current()
        .unwrap_or_else(|| fail("multi-caller B second token"));
    mailbox_empty();
    if second_b == call_b || operations.contains(&second_b)
        || ostd::ipc::reply(second_b, &[REQ_TAG, 65]).is_err() {
        fail("multi-caller B second reply");
    }
    raw_send(coordinator, &[CONTROL_TAG, 4]);
    sys_exit(0)
}

#[allow(dead_code)]
pub fn run_full_caller(role: &str) -> ! {
    let provider = role.strip_prefix("c2c-full-caller:")
        .and_then(|tid| tid.parse::<usize>().ok())
        .unwrap_or_else(|| fail("full caller provider argument"));
    let mut bytes = [0u8; 32];
    let coordinator = bounded_recv(0, &mut bytes);
    if bytes[..2] != [CONTROL_TAG, 0] || coordinator == provider {
        fail("full caller setup");
    }
    let mut calls = Vec::with_capacity(QUOTA);
    for sequence in 0..QUOTA {
        calls.push(ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, sequence as u8])
            .unwrap_or_else(|_| fail("full caller admission")));
        control_recv(provider, sequence as u8);
    }
    expect_busy(provider, 254);
    // Provider is gated before any A response: every accepted call stays pending.
    for call in &calls {
        if call.try_take(&mut bytes) != Ok(None) {
            fail("full caller completed before independent B");
        }
    }
    raw_send(coordinator, &[CONTROL_TAG, 0]);
    control_recv(coordinator, 1);
    control_recv(provider, 64);
    expect_busy(provider, 255);
    for (sequence, call) in calls.iter().enumerate() {
        take_reply(call, sequence as u8);
    }
    raw_send(coordinator, &[CONTROL_TAG, 2]);
    sys_exit(0)
}

fn multi_caller_leg() {
    let provider = spawn_probe("c2c-multi-provider")
        .unwrap_or_else(|_| fail("multi-caller provider spawn"));
    raw_send(provider, &[CONTROL_TAG, 0]);
    control_recv(provider, 0);
    let caller_a = spawn_probe(&format!("c2c-full-caller:{provider}"))
        .unwrap_or_else(|_| fail("full caller spawn"));
    raw_send(caller_a, &[CONTROL_TAG, 0]);
    control_recv(caller_a, 0);
    // A cannot drain until our later gate; B must progress with all 64 A slots held.
    let call_b = ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, 64])
        .unwrap_or_else(|_| fail("A owner quota blocked independent B"));
    take_reply(&call_b, 64);
    control_recv(provider, 2);
    raw_send(caller_a, &[CONTROL_TAG, 1]);
    raw_send(provider, &[CONTROL_TAG, 3]);
    control_recv(caller_a, 2);
    let second_b = ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, 65])
        .unwrap_or_else(|_| fail("multi-caller B progress after A drain"));
    take_reply(&second_b, 65);
    control_recv(provider, 4);
    println("[local-lifecycle] MULTI-CALLER=OK a_held=64 a_busy=2 a_drained=64 b_replies=2 wrong_correlation=0 busy_delivered=0");
}

type PressureSlots = [Option<(ostd::ipc::PendingCall, u8)>; QUOTA];

fn pump_pressure(provider: usize, producer: u8, total: u8, mut next: u8,
    mut slots: PressureSlots) -> (usize, u64) {
    let started = scheduler_ticks();
    let mut done = 0u8;
    let mut busy = 0usize;
    let mut last_progress = started;
    let mut max_gap = 0u64;
    let mut bytes = [0u8; 32];
    while done < total {
        for slot in &mut slots {
            if let Some((call, sequence)) = slot.as_ref() {
                if let Some(completion) = call.try_take(&mut bytes)
                    .unwrap_or_else(|_| fail("pressure take")) {
                    if completion.terminal != IpcTerminal::Reply || completion.len != 3
                        || bytes[..3] != [REQ_TAG, producer, *sequence]
                        || call.try_take(&mut bytes) != Err(ostd::ipc::IpcError::InvalidOperation) {
                        fail("pressure completion identity or duplicate");
                    }
                    let now = scheduler_ticks();
                    max_gap = max_gap.max(now.saturating_sub(last_progress));
                    last_progress = now;
                    done += 1;
                    *slot = None;
                }
            }
        }
        for slot in &mut slots {
            if slot.is_some() || next == total { continue; }
            match ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, producer, next]) {
                Ok(call) => {
                    *slot = Some((call, next));
                    next += 1;
                }
                Err(ostd::ipc::IpcSubmitError::Busy) => {
                    busy += 1;
                    break;
                }
                Err(_) => fail("pressure admission"),
            }
        }
        if scheduler_ticks().saturating_sub(started) >= 1_000 {
            fail("pressure producer progress watchdog");
        }
        if done < total { yield_now(); }
    }
    (busy, max_gap)
}

#[allow(dead_code)]
pub fn run_pressure_provider() -> ! {
    let mut bytes = [0u8; 32];
    let coordinator = bounded_recv(0, &mut bytes);
    if bytes[..2] != [CONTROL_TAG, 0] { fail("pressure provider setup"); }
    raw_send(coordinator, &[CONTROL_TAG, 0]);
    // A fills the queue and B proves refusal before this real timer expires.
    if !matches!(ostd::syscall::sys_set_timer(100), SyscallResult::Ok(0)) {
        fail("pressure provider hold");
    }
    let mut seen = [[false; 128]; 2];
    let mut counts = [0usize; 2];
    let mut caller_b = None;
    for _ in 0..192 {
        let sender = bounded_recv(0, &mut bytes);
        let producer = bytes[1] as usize;
        let sequence = bytes[2] as usize;
        if bytes[0] != REQ_TAG || producer > 1 || sequence >= if producer == 0 {128} else {64}
            || seen[producer][sequence] || (producer == 0 && sender != coordinator)
            || (producer == 1 && (sender == coordinator || caller_b.is_some_and(|tid| tid != sender))) {
            fail("pressure request identity or duplicate delivery");
        }
        if producer == 1 { caller_b = Some(sender); }
        seen[producer][sequence] = true;
        counts[producer] += 1;
        let operation = ostd::ipc::current().unwrap_or_else(|| fail("pressure dispatch token"));
        if ostd::ipc::reply(operation, &bytes[..3]).is_err() {
            fail("pressure exact reply");
        }
    }
    if counts != [128, 64] { fail("pressure missing producer work"); }
    mailbox_empty();
    println("[local-lifecycle] PRESSURE-PROVIDER=OK a=128 b=64 duplicate=0 extra_delivery=0");
    raw_send(coordinator, &[CONTROL_TAG, 3]);
    sys_exit(0)
}

#[allow(dead_code)]
pub fn run_pressure_caller(role: &str) -> ! {
    let provider = role.strip_prefix("c2c-pressure-caller:")
        .and_then(|tid| tid.parse::<usize>().ok())
        .unwrap_or_else(|| fail("pressure caller argument"));
    let mut bytes = [0u8; 32];
    let coordinator = bounded_recv(0, &mut bytes);
    if bytes[..2] != [CONTROL_TAG, 0] { fail("pressure caller setup"); }
    if !matches!(ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, 1, 0]),
        Err(ostd::ipc::IpcSubmitError::Busy)) {
        fail("full peer mailbox did not refuse independent caller");
    }
    raw_send(coordinator, &[CONTROL_TAG, 0]);
    control_recv(coordinator, 1);
    let (busy, max_gap) = pump_pressure(provider, 1, 64, 0, core::array::from_fn(|_| None));
    println(&format!("[local-lifecycle] PRESSURE-CALLER b_completed=64 busy={} max_gap_ticks={}",
        busy + 1, max_gap));
    raw_send(coordinator, &[CONTROL_TAG, 2]);
    sys_exit(0)
}

fn peer_pressure_leg() {
    let provider = spawn_probe("c2c-pressure-provider")
        .unwrap_or_else(|_| fail("pressure provider spawn"));
    let started = scheduler_ticks();
    raw_send(provider, &[CONTROL_TAG, 0]);
    control_recv(provider, 0);
    let mut slots: PressureSlots = core::array::from_fn(|_| None);
    for (sequence, slot) in slots.iter_mut().enumerate() {
        let call = ostd::ipc::PendingCall::submit(provider, &[REQ_TAG, 0, sequence as u8])
            .unwrap_or_else(|_| fail("pressure initial queue fill"));
        *slot = Some((call, sequence as u8));
    }
    let caller_b = spawn_probe(&format!("c2c-pressure-caller:{provider}"))
        .unwrap_or_else(|_| fail("pressure B spawn"));
    raw_send(caller_b, &[CONTROL_TAG, 0]);
    control_recv(caller_b, 0);
    if scheduler_ticks().saturating_sub(started) >= 50 {
        fail("pressure setup exceeded timer margin");
    }
    raw_send(caller_b, &[CONTROL_TAG, 1]);
    let (busy, max_gap) = pump_pressure(provider, 0, 128, 64, slots);
    control_recv(caller_b, 2);
    control_recv(provider, 3);
    println(&format!("[local-lifecycle] PRESSURE-CALLER a_completed=128 busy={busy} max_gap_ticks={max_gap}"));
    println("[local-lifecycle] PEER-PRESSURE=OK a=128 b=64 completions=192 initial_peer_busy=true duplicate=0 wrong_correlation=0");
}

// ── orchestrator role (bench) ─────────────────────────────────────────────────

fn settle() {
    for _ in 0..SETTLE_YIELDS {
        yield_now();
    }
}

/// Leg 0: the additive `LookupServiceBound = 429` resolves a real, init-registered
/// service to a full `(tid, cell_id, generation)` binding.
///
/// This is the end-to-end reachability proof for the opcode: allowlist bit 37, the
/// kernel dispatch, the registry's recorded identity, the fixed 24-byte write into a
/// caller buffer, and `ServiceBinding::from_bytes` all have to work for the anchors
/// below to appear. A capture bug (identity stored as 0/0) makes `lookup_bound`
/// report no binding, so this leg fails loudly instead of passing on a stub.
fn binding_leg() {
    let vfs_tid = sys_lookup_service(api::syscall::service::VFS);
    let mut buf = [0u8; api::service_binding::SERVICE_BINDING_LEN];
    match sys_lookup_service_bound(api::syscall::service::VFS, &mut buf) {
        Some(binding) => {
            println(&format!(
                "[local-lifecycle] VFS-BINDING tid={} cell={} gen={} matches_lookup={}",
                binding.tid,
                binding.cell_id,
                binding.generation,
                Some(binding.tid as usize) == vfs_tid
            ));
        }
        None => println("[local-lifecycle] VFS-BINDING=none"),
    }

    // No provider: this image neither builds nor launches the AI cell, so the id
    // resolves to nothing and the bound lookup must say so rather than write a
    // record. `0` from `LookupService` and "no binding" from
    // `LookupServiceBound` are the same absence, stated two ways.
    let absent_tid = sys_lookup_service(api::syscall::service::AI);
    let absent_binding = sys_lookup_service_bound(api::syscall::service::AI, &mut buf);
    println(&format!(
        "[local-lifecycle] ABSENT-BINDING lookup={:?} bound_is_none={}",
        absent_tid,
        absent_binding.is_none()
    ));
}

/// Leg A: the synchronous SDK caller must return on provider death.
fn sync_leg() {
    let provider = match spawn_probe(PROVIDER_ROLE) {
        Ok(tid) => tid,
        Err(()) => fail("sync-leg provider spawn"),
    };
    settle();

    let _caller = match spawn_probe(&format!("{SYNC_CALLER_PREFIX}{provider}")) {
        Ok(tid) => tid,
        Err(()) => fail("sync-leg caller spawn"),
    };
    settle();

    println("[local-lifecycle] SYNC-LEG=complete");
}

/// Leg B: the bounded exact-operation path reports a terminal instead of hanging.
fn async_leg() {
    let provider = match spawn_probe(PROVIDER_ROLE) {
        Ok(tid) => tid,
        Err(()) => fail("async-leg provider spawn"),
    };
    settle();

    let token = match submit(provider, &[REQ_TAG]) {
        Ok(token) => token,
        Err(_) => fail("async-leg submit"),
    };

    let mut reply = [0u8; IPC_BUF_SIZE];
    let mut terminal = None;
    for _ in 0..ASYNC_WAIT_ROUNDS {
        if !wait(ASYNC_WAIT_TICKS) {
            continue;
        }
        match take(token, &mut reply) {
            Ok(IpcTakeResult::Terminal { status, .. }) => {
                terminal = Some(status);
                break;
            }
            Ok(IpcTakeResult::Pending) => continue,
            Err(_) => break,
        }
    }

    match terminal {
        Some(IpcTerminal::PeerGone) => {
            println("[local-lifecycle] ASYNC-TERMINAL=PEER-GONE");
        }
        Some(other) => println(&format!("[local-lifecycle] ASYNC-TERMINAL={other:?}")),
        None => println("[local-lifecycle] ASYNC-TERMINAL=NONE"),
    }
}

/// Leg S: the shipped SDK resolves through the frozen binding, and refuses when no live
/// binding exists.
///
/// This is the caller-side half of ADR-0023: the kernel returned a binding, but nothing
/// in the SDK consumed it. `ServiceRef` — the caching handle cells and clients hold — must
/// resolve the *same* binding the raw opcode reports, a typed call through it must still
/// work, and `LocalEndpoint::bind()` must refuse a service with no live provider rather
/// than send to a tid someone once wrote down.
///
/// The stale-after-death half of the rule needs a provider that is registered and then
/// dies; no fixture in this image does that, so it is covered by
/// `ostd::service::classify_call_failure`'s host tests and becomes a lane witness when the
/// Phase-02 cross-tier fixture registers a service and is killed.
fn sdk_leg() {
    let mut record = [0u8; api::service_binding::SERVICE_BINDING_LEN];
    let raw = sys_lookup_service_bound(api::syscall::service::VFS, &mut record);

    let mut vfs: ServiceRef<{ api::syscall::service::VFS }> = ServiceRef::new();
    let sdk = vfs.binding();
    println(&format!(
        "[local-lifecycle] SDK-BINDING matches_raw={} tid={:?}",
        sdk == raw,
        sdk.map(|binding| binding.tid)
    ));
    if sdk != raw {
        fail("sdk-leg: ServiceRef binding disagrees with the raw lookup");
    }

    // The handle resolves, sends, waits for the masked reply and decodes it — all keyed on
    // the binding it resolved, not on a tid it was handed.
    let mut response_buffer = [0u8; IPC_BUF_SIZE];
    match vfs.call::<VfsRequest, VfsResponse>(&VfsRequest::Stat("/"), &mut response_buffer) {
        Ok(VfsResponse::Stat { .. }) => println("[local-lifecycle] SDK-VFS-CALL=OK"),
        Ok(other) => {
            println(&format!(
                "[local-lifecycle] SDK-VFS-CALL=UNEXPECTED {other:?}"
            ));
            fail("sdk-leg: VFS Stat answered with a different response");
        }
        Err(error) => {
            println(&format!("[local-lifecycle] SDK-VFS-CALL=ERR {error:?}"));
            fail("sdk-leg: ServiceRef call to a live VFS failed");
        }
    }

    // The binding the handle holds is still the live one, and the unresolved case says so
    // without querying: an unbound handle is not live.
    let mut absent = ServiceRef::<{ api::syscall::service::AI }>::new();
    let resolved_still_live = vfs.is_live();
    let unresolved_live = absent.is_live();
    println(&format!(
        "[local-lifecycle] SDK-BINDING-LIVE resolved={} unresolved={}",
        resolved_still_live, unresolved_live
    ));
    if !resolved_still_live {
        fail("sdk-leg: a resolved live binding reported itself stale");
    }
    if unresolved_live {
        fail("sdk-leg: an unresolved handle reported itself live");
    }

    // No live provider is a refusal, not a tid to reuse.
    match LocalEndpoint::<AbsentService>::bind() {
        Err(EndpointError::NoLiveBinding) => {
            println("[local-lifecycle] SDK-ABSENT-BINDING=REFUSED")
        }
        Err(other) => {
            println(&format!("[local-lifecycle] SDK-ABSENT-BINDING={other:?}"));
            fail("sdk-leg: absent-service bind must report NoLiveBinding");
        }
        Ok(_) => fail("sdk-leg: absent-service bind must not succeed"),
    }
}

/// Typed method for a service this image does not run. Only the id matters here: the
/// refusal has to happen before any payload is built.
struct AbsentService;

impl CellMethod for AbsentService {
    type Request = ();
    type Response<'a> = ();

    const SERVICE_ID: u16 = api::syscall::service::AI;
    const EXPORT_ID: u16 = 0;
    const RETRY_CLASS: RetryClass = RetryClass::Idempotent;
}

pub fn run() -> ! {
    println("[local-lifecycle] START");
    binding_leg();
    sdk_leg();
    sync_leg();
    nested_reply_leg();
    sdk_slot_cleanup_leg();
    cancelled_reply_leg();
    async_leg();
    saturation_leg();
    deadline_leg();
    caller_death_leg();
    restart_leg();
    event_coexistence_leg();
    multi_caller_leg();
    peer_pressure_leg();
    println("[local-lifecycle] PASS");
    sys_exit(0)
}
