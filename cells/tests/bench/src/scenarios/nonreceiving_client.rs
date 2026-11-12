//! Non-receiving client witness (architecture remediation Phase 11, CP1 §4.1).
//!
//! A client sends a request by raw `Send` to the supervisor, config or
//! compositor and then never receives. The service's reply must not park it:
//! a second client's bounded call is answered within [`SECOND_CALL_TICKS`].
//! Before services replied without parking, the service sat in `Sending` until
//! the first client exited, and the second call timed out.

use alloc::format;
use api::ipc::IPC_BUF_SIZE;
use api::services::ipc::ConfigRequest;
use api::syscall::service;
use api::task::TaskPriority;
use ostd::{
    io::println,
    ipc::{IpcTerminal, PendingCall},
    syscall::{
        sys_exit, sys_get_scheduler_ticks, sys_lookup_service, sys_recv_timeout, sys_send,
        sys_set_spawn_args, sys_spawn_pinned, sys_try_send, SyscallResult,
    },
    task::yield_now,
};

const PROBE_PATH: &str = "/bin/bench-probe";
const CLIENT_PREFIX: &str = "nr-client:";
const CONTROL: [u8; 2] = [0xC0, 0x4E];
/// The first client stays alive, never receiving, well past the second call.
const CLIENT_HOLD_TICKS: u64 = 1_000;
/// Handshake watchdog for the first client's "request accepted" notice.
const HANDSHAKE_TICKS: u64 = 500;
/// Budget for the second client's answer; 10 ms ticks.
const SECOND_CALL_TICKS: u64 = 300;

const SUPERVISOR_ENVELOPE: [u8; 2] = [0xAC, 0x00];
/// `OP_SNAPSHOT`: no caller is reviewed for it, so every caller gets a refusal.
const SUPERVISOR_SNAPSHOT: u8 = 0x02;
/// `encode_status(0, STATUS_REJECTED_CALLER)`.
const SUPERVISOR_REFUSAL: [u8; 3] = [0x03, 0x00, 0xFD];

fn ticks() -> u64 {
    sys_get_scheduler_ticks().unwrap_or_else(|| fail("scheduler clock unavailable"))
}

fn fail(reason: &str) -> ! {
    println(&format!("[nonreceiving] FAIL — {reason}"));
    sys_exit(1)
}

/// The request every client sends to `service_id`, in `buf`.
fn request(service_id: u16, buf: &mut [u8; IPC_BUF_SIZE]) -> usize {
    match service_id {
        service::CONFIG => api::ipc::encode(&ConfigRequest::Get("OS"), buf)
            .map(|bytes| bytes.len())
            .unwrap_or_else(|_| fail("config request encode")),
        service::SUPERVISOR => {
            buf[..2].copy_from_slice(&SUPERVISOR_ENVELOPE);
            buf[2] = SUPERVISOR_SNAPSHOT;
            3
        }
        service::COMPOSITOR => {
            buf[0] = api::display::compositor_ops::GET_SCREEN_SIZE;
            1
        }
        _ => fail("unknown service"),
    }
}

/// Whether `reply` is the answer `service_id` gives to [`request`].
fn reply_matches(service_id: u16, reply: &[u8]) -> bool {
    match service_id {
        service::CONFIG => matches!(
            api::ipc::decode::<api::services::ipc::ConfigResponse>(reply),
            Ok(api::services::ipc::ConfigResponse::Value("Cellos"))
        ),
        service::SUPERVISOR => reply == SUPERVISOR_REFUSAL,
        service::COMPOSITOR => reply.len() == 8,
        _ => false,
    }
}

const fn name(service_id: u16) -> &'static str {
    match service_id {
        service::CONFIG => "config",
        service::SUPERVISOR => "supervisor",
        service::COMPOSITOR => "compositor",
        _ => "unknown",
    }
}

// ── bench-probe role ──────────────────────────────────────────────────────────

/// Raw non-blocking notice to `peer`, retried until it is receiving.
fn notify(peer: usize) {
    let start = ticks();
    loop {
        match sys_try_send(peer, &CONTROL) {
            SyscallResult::Ok(0) => return,
            SyscallResult::Ok(usize::MAX) => {}
            _ => fail("control notice"),
        }
        if ticks().saturating_sub(start) >= HANDSHAKE_TICKS {
            fail("control notice watchdog");
        }
        yield_now();
    }
}

/// First client: learn the orchestrator from its kickoff, send one request by
/// raw `Send`, tell the orchestrator it was accepted, then stay alive without
/// ever receiving again, and exit.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_client(role: &str) -> ! {
    let Some(Ok(service_id)) = role.strip_prefix(CLIENT_PREFIX).map(str::parse::<u16>) else {
        fail("client argument")
    };
    let mut kickoff = [0u8; 8];
    let parent = match sys_recv_timeout(0, &mut kickoff, HANDSHAKE_TICKS) {
        SyscallResult::Ok(sender) if sender != 0 && kickoff[..2] == CONTROL => sender,
        _ => fail("client kickoff"),
    };
    let target = sys_lookup_service(service_id).unwrap_or_else(|| fail("client lookup"));
    let mut buf = [0u8; IPC_BUF_SIZE];
    let len = request(service_id, &mut buf);
    // Returns once the service has taken the request.
    if !matches!(sys_send(target, &buf[..len]), SyscallResult::Ok(_)) {
        fail("client send");
    }
    notify(parent);
    let start = ticks();
    while ticks().saturating_sub(start) < CLIENT_HOLD_TICKS {
        yield_now();
    }
    sys_exit(0)
}

// ── orchestrator ──────────────────────────────────────────────────────────────

fn spawn_client(service_id: u16) -> usize {
    if !sys_set_spawn_args(&format!("{CLIENT_PREFIX}{service_id}")) {
        fail("spawn arguments");
    }
    match sys_spawn_pinned(PROBE_PATH, TaskPriority::Normal as u8, 0) {
        SyscallResult::Ok(tid) if tid != 0 => tid,
        _ => fail("spawn client"),
    }
}

/// One leg: a parked first client, then a bounded second call.
fn leg(service_id: u16) {
    if sys_lookup_service(service_id).is_none() {
        println(&format!("[nonreceiving] {} ABSENT", name(service_id)));
        return;
    }
    let client = spawn_client(service_id);
    notify(client);
    let mut notice = [0u8; 8];
    match sys_recv_timeout(client, &mut notice, HANDSHAKE_TICKS) {
        SyscallResult::Ok(sender) if sender == client && notice[..2] == CONTROL => {}
        _ => fail("first client never reached the service"),
    }
    let target = sys_lookup_service(service_id).unwrap_or_else(|| fail("lookup"));
    let mut buf = [0u8; IPC_BUF_SIZE];
    let len = request(service_id, &mut buf);
    let call = PendingCall::submit(target, &buf[..len]).unwrap_or_else(|_| fail("submit"));
    let mut reply = [0u8; IPC_BUF_SIZE];
    let completion = match call.wait_and_take(&mut reply, SECOND_CALL_TICKS, 1) {
        Ok(Some(completion)) => completion,
        Ok(None) => {
            let _ = call.cancel();
            fail(&format!(
                "{} parked on a client that does not receive",
                name(service_id)
            ))
        }
        Err(_) => fail("second call take"),
    };
    if completion.terminal != IpcTerminal::Reply
        || !reply_matches(service_id, &reply[..completion.len])
    {
        fail(&format!("{} second call outcome", name(service_id)));
    }
    println(&format!(
        "[nonreceiving] {} SERVED second=Reply first_client_receiving=false",
        name(service_id)
    ));
}

pub fn run() {
    println("[nonreceiving] START");
    for service_id in [service::CONFIG, service::SUPERVISOR, service::COMPOSITOR] {
        leg(service_id);
    }
    println("[nonreceiving] PASS");
}
