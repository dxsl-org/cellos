use crate::relay::RelayClient;
use ostd::sync::Mutex;
use ostd::syscall::{sys_recv_attested, sys_spawn, sys_try_send, SyscallResult};
use service_net_broker::local_ingress::parse_request;
use service_net_broker::local_queue::{
    BrokerState, CompletionError, IngressDecision, QueuedReply, REPLY_TRY_SEND_BUDGET,
    WORKER_HEARTBEAT_MS,
};
use service_net_broker::reply_pump::{dispatch_or_queue, pump_turn, TrySendResult};

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
    expect_spawn("local-worker", worker_entry);
    expect_spawn("reply-pump", reply_entry);
    expect_spawn("network-poller", network_entry);
}

fn expect_spawn(name: &str, entry: extern "C" fn(usize)) {
    match sys_spawn(entry as usize, 0) {
        SyscallResult::Ok(_) => {}
        SyscallResult::Err(_) => panic!("[net-broker] required role spawn failed: {name}"),
    }
}

extern "C" fn worker_entry(_arg: usize) {
    loop {
        ostd::syscall::sys_heartbeat(WORKER_HEARTBEAT_MS);
        let next = { BROKER_STATE.lock().take_next_request() };
        match next {
            Some(request) => {
                let reply = QueuedReply::success(&request);
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
    loop {
        ostd::syscall::sys_heartbeat(NETWORK_HEARTBEAT_MS);
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
