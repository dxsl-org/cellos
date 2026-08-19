use crate::relay::RelayClient;
use ostd::sync::Mutex;
use ostd::syscall::{sys_get_time_ms, sys_recv_attested, sys_spawn, sys_try_send, SyscallResult};
use service_net_broker::bench_oracle::{
    self, encode_hold_command, encode_snapshot_payload, OracleCommand, STATIC_FOOTPRINT_BYTES,
};
use service_net_broker::local_ingress::{parse_request, ReplyStatus, MAX_REPLY_BODY};
use service_net_broker::local_queue::{
    BrokerState, CompletionError, IngressDecision, QueuedReply, REPLY_TRY_SEND_BUDGET,
    WORKER_HEARTBEAT_MS,
};
use service_net_broker::local_runtime_metrics::heartbeat_gap_miss;
use service_net_broker::reply_pump::{dispatch_or_queue, pump_turn, TrySendResult};
use service_net_broker::runtime_roles::{start_runtime_roles, RuntimeRole};

const IPC_BUF_SIZE: usize = api::ipc::IPC_BUF_SIZE;
const NETWORK_HEARTBEAT_MS: u64 = 500;

static BROKER_STATE: Mutex<BrokerState> = Mutex::new(BrokerState::new());
static RELAY_CLIENT: Mutex<Option<RelayClient>> = Mutex::new(None);

pub fn init(relay_client: RelayClient) {
    *RELAY_CLIENT.lock() = Some(relay_client);
    start_runtime_threads();
}

pub fn receive_once() {
    let mut buf = [0u8; IPC_BUF_SIZE];
    match sys_recv_attested(0, &mut buf) {
        SyscallResult::Ok(sender) if sender > 0 => {
            let immediate = {
                let mut state = BROKER_STATE.lock();
                let identity = api::caller_identity::CallerIdentity::from_recv_buf(&buf);
                state.handle_ingress(sender, identity, parse_request(&buf))
            };
            if let IngressDecision::Immediate(reply) = immediate {
                let mut state = BROKER_STATE.lock();
                dispatch_or_queue(&mut state, reply, try_send_reply);
            }
        }
        _ => ostd::task::yield_now(),
    }
}

fn start_runtime_threads() {
    if let Err(role) = start_runtime_roles(|role| match spawn_role(role) {
        SyscallResult::Ok(_) => Ok(()),
        SyscallResult::Err(_) => Err(()),
    }) {
        panic!("[net-broker] required role spawn failed: {}", role.name());
    }
}

extern "C" fn worker_entry(_arg: usize) {
    loop {
        ostd::syscall::sys_heartbeat(WORKER_HEARTBEAT_MS);
        let next = { BROKER_STATE.lock().take_next_request() };
        match next {
            Some(request) => {
                let reply = process_request(&request);
                loop {
                    match BROKER_STATE.lock().complete_request(&request, reply) {
                        Ok(()) => break,
                        Err(CompletionError::ReplyQueueFull) => ostd::task::yield_now(),
                        Err(CompletionError::Stale) => break,
                    }
                }
            }
            None => ostd::task::yield_now(),
        }
    }
}

extern "C" fn reply_entry(_arg: usize) {
    loop {
        ostd::syscall::sys_heartbeat(WORKER_HEARTBEAT_MS);
        let mut state = BROKER_STATE.lock();
        let progressed = pump_turn(&mut state, REPLY_TRY_SEND_BUDGET, try_send_reply);
        drop(state);
        if !progressed {
            ostd::task::yield_now();
        }
    }
}

extern "C" fn network_entry(_arg: usize) {
    let mut last_poll_ms = None;
    loop {
        ostd::syscall::sys_heartbeat(NETWORK_HEARTBEAT_MS);
        let now_ms = sys_get_time_ms();
        if heartbeat_gap_miss(last_poll_ms, now_ms) {
            BROKER_STATE.lock().note_heartbeat_miss();
        }
        last_poll_ms = now_ms;
        if let Some(relay_client) = RELAY_CLIENT.lock().as_ref() {
            let _ = relay_client.is_connected();
        }
        BROKER_STATE.lock().note_network_poll();
        ostd::task::yield_now();
    }
}

fn try_send_reply(reply: &QueuedReply) -> TrySendResult {
    let mut buf = [0u8; IPC_BUF_SIZE];
    let len = reply.encode(&mut buf);
    match sys_try_send(reply.caller_tid(), &buf[..len]) {
        SyscallResult::Ok(value) if value == usize::MAX => TrySendResult::Busy,
        SyscallResult::Ok(_) => TrySendResult::Delivered,
        SyscallResult::Err(_) => TrySendResult::Busy,
    }
}

fn spawn_role(role: RuntimeRole) -> SyscallResult {
    let entry = match role {
        RuntimeRole::LocalWorker => worker_entry,
        RuntimeRole::ReplyPump => reply_entry,
        RuntimeRole::NetworkPoller => network_entry,
    };
    sys_spawn(entry as usize, 0)
}

fn process_request(request: &service_net_broker::local_queue::WorkerRequest) -> QueuedReply {
    match bench_oracle::parse_command(&request.payload[..request.payload_len]) {
        Ok(OracleCommand::Echo(_)) => reply_with_payload(
            request,
            ReplyStatus::Success,
            &request.payload[..request.payload_len],
        ),
        Ok(OracleCommand::Snapshot) => {
            let mut payload = [0u8; MAX_REPLY_BODY];
            let mut snapshot = [0u8; bench_oracle::SNAPSHOT_BYTES];
            let counters = BROKER_STATE.lock().counters;
            encode_snapshot_payload(&counters, STATIC_FOOTPRINT_BYTES as u64, &mut snapshot);
            payload[..snapshot.len()].copy_from_slice(&snapshot);
            reply_with_payload(request, ReplyStatus::Success, &payload[..snapshot.len()])
        }
        Ok(OracleCommand::Hold { work_turns }) => {
            run_bounded_hold(work_turns);
            let mut payload = [0u8; 3];
            let len = encode_hold_command(work_turns, &mut payload).unwrap_or(0);
            reply_with_payload(request, ReplyStatus::Success, &payload[..len])
        }
        Err(_) => reply_with_payload(request, ReplyStatus::Indeterminate, &[]),
    }
}

fn run_bounded_hold(work_turns: u16) {
    for _ in 0..work_turns {
        ostd::syscall::sys_heartbeat(WORKER_HEARTBEAT_MS);
        ostd::task::yield_now();
    }
}

fn reply_with_payload(
    request: &service_net_broker::local_queue::WorkerRequest,
    status: ReplyStatus,
    payload: &[u8],
) -> QueuedReply {
    QueuedReply::new(
        request.key.caller_tid,
        request.key.request_id,
        request.client_sequence,
        status,
        payload,
        request.order,
    )
}
