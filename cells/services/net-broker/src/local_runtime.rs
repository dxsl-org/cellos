use crate::beacon::{self, BeaconChannel, BeaconPlain, PeerTable};
use crate::rng::BrokerRng;
use crate::transport::StaticKeypair;
use api::cluster::ClusterId;
use ostd::service::NetRef;
use ostd::sync::{Mutex, MutexGuard};
use ostd::syscall::{
    sys_get_time, sys_get_time_ms, sys_recv_attested, sys_spawn, sys_try_send, SyscallResult,
};
use service_net_broker::bench_oracle;
use service_net_broker::identity::BrokerIdentity;
use service_net_broker::local_ingress::parse_request;
use service_net_broker::local_queue::{
    BrokerState, CompletionError, IngressDecision, QueuedReply, REPLY_TRY_SEND_BUDGET,
    WORKER_HEARTBEAT_TICKS,
};
use service_net_broker::local_runtime_metrics::heartbeat_gap_miss;
use service_net_broker::reply_pump::{retain_busy_reply, RetainBusyResult, TrySendResult};
use service_net_broker::runtime_roles::{start_runtime_roles, RuntimeRole};

#[path = "local_runtime/request_dispatch.rs"]
mod request_dispatch;
#[cfg(feature = "restart-oracle")]
#[path = "local_runtime/restart_oracle.rs"]
mod restart_oracle;

use request_dispatch::process_request;

const IPC_BUF_SIZE: usize = api::ipc::IPC_BUF_SIZE;
// This broker-local deadline covers the bounded 512-turn saturation probe plus
// scheduler margin; it does not relax the kernel's global heartbeat policy.
const NETWORK_HEARTBEAT_TICKS: u64 = 1_000;

struct RuntimeState {
    broker: BrokerState,
    network: Option<BrokerNetworkState>,
}

impl RuntimeState {
    const fn new() -> Self {
        Self {
            broker: BrokerState::new(),
            network: None,
        }
    }
}

pub struct BrokerNetworkState {
    gossip_key: [u8; 32],
    identity: BrokerIdentity,
    _static_keypair: StaticKeypair,
    rng: BrokerRng,
    cluster_id: u64,
    machine_id: u64,
    boot_epoch: u64,
    beacon_counter: u64,
    next_beacon_at: u64,
    channel: BeaconChannel,
    net: NetRef,
    peers: PeerTable,
}

impl BrokerNetworkState {
    pub fn initialize(
        k1: &[u8; 32],
        identity: BrokerIdentity,
        static_keypair: StaticKeypair,
        rng: BrokerRng,
    ) -> Option<Self> {
        let gossip_key = beacon::derive_gossip_key(k1);
        let cluster_id = ClusterId::from_name("robots").0;
        let machine_id = beacon::derive_machine_id(&identity.node_id);
        let boot_epoch = sys_get_time_ms()?;
        let mut net = NetRef::new();
        let channel = BeaconChannel::init(&mut net)?;
        Some(Self {
            gossip_key,
            identity,
            _static_keypair: static_keypair,
            rng,
            cluster_id,
            machine_id,
            boot_epoch,
            beacon_counter: 0,
            next_beacon_at: boot_epoch,
            channel,
            net,
            peers: PeerTable::new(),
        })
    }

    fn poll_beacon(&mut self) {
        let Some(now) = sys_get_time_ms() else {
            return;
        };
        if beacon::beacon_due(now, self.next_beacon_at) {
            self.next_beacon_at = beacon::next_beacon_deadline(now);
            if self.beacon_counter != u64::MAX {
                let plain = BeaconPlain::local(
                    self.cluster_id,
                    self.machine_id,
                    self.boot_epoch,
                    self.beacon_counter,
                );
                let frame = beacon::encrypt_beacon(&self.gossip_key, &plain, &mut self.rng);
                if self.channel.send_frame(&mut self.net, &frame) {
                    self.beacon_counter += 1;
                }
            }
        }

        let Some(frame) = self.channel.try_recv_frame(&mut self.net) else {
            return;
        };
        let Some(plain) = beacon::decrypt_beacon(&self.gossip_key, &frame) else {
            return;
        };
        if !self.accepts_beacon(&plain) {
            return;
        }
        self.peers.update(&plain);
    }

    fn accepts_beacon(&self, plain: &BeaconPlain) -> bool {
        beacon::accepts_peer_beacon(plain, self.cluster_id, self.machine_id, |machine_id| {
            (0..self.identity.peer_count()).any(|index| {
                self.identity
                    .get_peer(index)
                    .map(|peer| beacon::derive_machine_id(&peer.node_id) == machine_id)
                    .unwrap_or(false)
            })
        })
    }
}

static RUNTIME_STATE: Mutex<RuntimeState> = Mutex::new(RuntimeState::new());

fn lock_runtime_state(rearm_heartbeat: bool) -> MutexGuard<'static, RuntimeState> {
    loop {
        if let Some(state) = RUNTIME_STATE.try_lock() {
            return state;
        }
        if rearm_heartbeat {
            ostd::syscall::sys_heartbeat(WORKER_HEARTBEAT_TICKS);
        }
        ostd::task::yield_now();
    }
}

pub fn init(network: BrokerNetworkState) {
    lock_runtime_state(false).network = Some(network);
    start_runtime_threads();
}

pub fn receive_once() {
    let mut buf = [0u8; IPC_BUF_SIZE];
    match sys_recv_attested(0, &mut buf) {
        SyscallResult::Ok(sender) if sender > 0 => {
            let identity = api::caller_identity::CallerIdentity::from_recv_buf(&buf);
            let parsed = parse_request(&buf);
            #[cfg(feature = "restart-oracle")]
            if restart_oracle::request_matches(sender, identity.as_ref(), parsed.as_ref().ok()) {
                restart_oracle::shutdown();
            }
            let immediate = {
                let mut state = lock_runtime_state(false);
                state.broker.handle_ingress(sender, identity, parsed)
            };
            if let IngressDecision::Immediate(reply) = immediate {
                send_or_queue(reply, false);
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
        #[cfg(feature = "restart-oracle")]
        restart_oracle::exit_role_if_requested();
        ostd::syscall::sys_heartbeat(WORKER_HEARTBEAT_TICKS);
        let next = { lock_runtime_state(true).broker.take_next_request() };
        match next {
            Some(request) => {
                let reply = process_request(&request);
                loop {
                    let completion = {
                        let mut state = lock_runtime_state(true);
                        state.broker.complete_request(&request, reply)
                    };
                    match completion {
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
        #[cfg(feature = "restart-oracle")]
        restart_oracle::exit_role_if_requested();
        ostd::syscall::sys_heartbeat(WORKER_HEARTBEAT_TICKS);
        pump_reply_turn();
        ostd::task::yield_now();
    }
}

extern "C" fn network_entry(_arg: usize) {
    let mut last_poll_ms = None;
    loop {
        #[cfg(feature = "restart-oracle")]
        restart_oracle::exit_role_if_requested();
        ostd::syscall::sys_heartbeat(NETWORK_HEARTBEAT_TICKS);
        let now_ms = sys_get_time_ms();
        if heartbeat_gap_miss(last_poll_ms, now_ms) {
            lock_runtime_state(true).broker.note_heartbeat_miss();
        }
        last_poll_ms = now_ms;

        // NetRef::call waits for a service response. Move the complete beacon
        // state out first so ingress and worker roles never wait on network IPC.
        let network = { lock_runtime_state(true).network.take() };
        if let Some(mut network) = network {
            network.poll_beacon();
            lock_runtime_state(true).network = Some(network);
        }
        lock_runtime_state(true).broker.note_network_poll();
        ostd::task::yield_now();
    }
}

fn try_send_reply(reply: &QueuedReply) -> TrySendResult {
    let mut buf = [0u8; IPC_BUF_SIZE];
    let len = reply.encode(&mut buf);
    let _ = bench_oracle::stamp_timed_echo_reply_frame(&mut buf[..len], sys_get_time());
    match sys_try_send(reply.caller_tid(), &buf[..len]) {
        SyscallResult::Ok(value) if value == usize::MAX => TrySendResult::Busy,
        SyscallResult::Ok(_) => TrySendResult::Delivered,
        SyscallResult::Err(_) => TrySendResult::Busy,
    }
}

fn send_or_queue(mut reply: QueuedReply, rearm_heartbeat: bool) {
    loop {
        if try_send_reply(&reply) == TrySendResult::Delivered {
            return;
        }
        let retention = {
            let mut state = lock_runtime_state(rearm_heartbeat);
            retain_busy_reply(&mut state.broker, reply)
        };
        match retention {
            RetainBusyResult::Queued | RetainBusyResult::Exhausted => return,
            RetainBusyResult::Saturated(still_pending) => {
                reply = still_pending;
                ostd::task::yield_now();
            }
        }
    }
}

fn pump_reply_turn() {
    for _ in 0..REPLY_TRY_SEND_BUDGET {
        let next = { lock_runtime_state(true).broker.take_next_reply() };
        let Some(reply) = next else {
            break;
        };
        send_or_queue(reply, true);
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
