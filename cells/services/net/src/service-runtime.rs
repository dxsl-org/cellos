use alloc::{collections::BTreeMap, string::String};
use api::ipc::{self, NetRequest, NetResponse};
use api::syscall::events::NET_RX;
use core::sync::atomic::{AtomicU16, Ordering};
#[cfg(any(not(feature = "ipc-wake-oracle"), feature = "hypervisor-bridge"))]
use ostd::syscall::sys_wait_completion;
use ostd::syscall::{sys_try_recv_attested, sys_yield};
#[cfg(all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge")))]
use ostd::syscall::{sys_wait_completion_detailed, WaitCompletionResult};
use ostd::{
    io::println,
    syscall::{sys_get_time, SyscallResult},
};
use smoltcp::{
    iface::{Config, Interface, SocketSet, SocketStorage},
    time::Instant,
    wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr},
};

use crate::{
    dhcp::{add_dhcp_socket, poll_dhcp, DhcpState},
    dns::Resolver,
    handlers,
    handlers::tcp::{readiness, MAX_INTERESTS},
    interface::VirtioNetDevice,
    socket_table::{SocketOwner, SocketTable, SOCKET_SET_STORAGE},
    tls::socket::TlsSocketEntry,
};

const IPC_BUF_SIZE: usize = 4096;
const MAC: EthernetAddress = EthernetAddress([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
const NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS: u64 = 10;
#[cfg(any(
    test,
    all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge"))
))]
const SCHEDULER_TICK_MS: u64 = 10;
const IPC_BURST_GRACE_YIELDS: u8 = 1;
const SMOLTCP_MAINTENANCE_INTERVAL_MS: u64 = 100;
const MTIME_TICKS_PER_MS: u64 = 10_000;
const SMOLTCP_MAINTENANCE_TICKS: u64 = SMOLTCP_MAINTENANCE_INTERVAL_MS * MTIME_TICKS_PER_MS;
/// One `[net-loop]` line per this many raw timer ticks (`loop-trace` images).
///
/// It used to be ten times shorter (`200 * MTIME_TICKS_PER_MS`), which made the
/// trace print about once per loop turn — the noise an operator notices first,
/// and enough UART time to change the thing it was measuring. The tick rate is
/// per-arch, and `MTIME_TICKS_PER_MS` is only a 10 MHz assumption, so this is a
/// nominal 2 s: measured on QEMU raspi3b it lands at about one line per four
/// turns (8962 → 2245 lines over the same 900 s window, same ~8960 turns).
const LOOP_TRACE_INTERVAL_TICKS: u64 = 2_000 * MTIME_TICKS_PER_MS;
// A 10-tick deadline can fire just after 9 complete tick periods when the
// submission lands immediately before a scheduler tick. The exclusive ceiling
// therefore has to be the preceding 9 periods, not the 100 ms maintenance budget.
#[cfg(any(
    test,
    all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge"))
))]
const IDLE_IPC_WAKE_PROOF_CEILING_TICKS: u64 =
    (NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS - 1) * SCHEDULER_TICK_MS * MTIME_TICKS_PER_MS;

#[cfg(any(
    test,
    all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge"))
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IdleIpcWakeClassification {
    Pass,
    Inconclusive,
}

/// Classify the ticks a recordless wait burned *before it returned*.
///
/// `wait_return_ticks` is the wait's own latency, not the latency of the IPC
/// that followed it: only the return can be below the exclusive ceiling (a
/// deadline return lands at whole tick periods, at or above it), and the
/// waiter's path back to `TryRecv` afterwards is not part of the wake.
#[cfg(any(
    test,
    all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge"))
))]
pub(crate) const fn classify_idle_ipc_wake(wait_return_ticks: u64) -> IdleIpcWakeClassification {
    if wait_return_ticks < IDLE_IPC_WAKE_PROOF_CEILING_TICKS {
        IdleIpcWakeClassification::Pass
    } else {
        IdleIpcWakeClassification::Inconclusive
    }
}
static NEXT_PORT: AtomicU16 = AtomicU16::new(49152);
/// One-shot log guard for a refused owner death watch.
static WATCH_REFUSED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub(crate) fn next_ephemeral_port() -> u16 {
    let port = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
    if port >= 65534 {
        NEXT_PORT.store(49152, Ordering::Relaxed);
    }
    port
}

pub(crate) fn now_instant() -> Instant {
    Instant::from_micros((sys_get_time() / 10) as i64)
}

/// Dotted-quad text for log lines (`10.0.2.15`).
pub(crate) fn dotted_ipv4(ip: [u8; 4]) -> String {
    let mut text = String::new();
    for (index, octet) in ip.iter().enumerate() {
        if index > 0 {
            text.push('.');
        }
        let mut number = *octet as u32;
        let mut digits = [0u8; 3];
        let mut digit_index = 3;
        loop {
            digit_index -= 1;
            digits[digit_index] = b'0' + (number % 10) as u8;
            number /= 10;
            if number == 0 {
                break;
            }
        }
        for digit in &digits[digit_index..] {
            text.push(*digit as char);
        }
    }
    text
}

fn consume_ipc_burst_grace(remaining: &mut u8) -> bool {
    if *remaining == 0 {
        return false;
    }
    *remaining -= 1;
    true
}

const DNS_QUERY_LIMIT: usize = 16;
const DNS_QUERY_DEADLINE_TICKS: u64 = 30_000_000;

struct PendingDns {
    owner: SocketOwner,
    sender: usize,
    op_id: Option<usize>,
    query: Option<smoltcp::socket::dns::QueryHandle>,
    answer: Option<Option<[u8; 4]>>,
    deadline: u64,
}

fn handle_resolve(
    buffer: &[u8],
    sender: usize,
    owner: SocketOwner,
    iface: &mut Interface,
    sockets: &mut SocketSet<'_>,
    resolver: &Resolver,
    pending: &mut heapless::Vec<PendingDns, DNS_QUERY_LIMIT>,
) -> bool {
    let Ok(NetRequest::Resolve { hostname }) = ipc::decode::<NetRequest<'_>>(buffer) else {
        return false;
    };
    if let Some(ip) = crate::dns::static_lookup(hostname, resolver.server()) {
        send_ready(sender, NetResponse::Addr(ip));
        return true;
    }
    if pending.is_full() {
        send_ready(sender, NetResponse::Err(0xFD));
        return true;
    }
    let Some(query) = resolver.start(hostname, iface, sockets) else {
        send_ready(sender, NetResponse::Err(0xFF));
        return true;
    };
    // Legacy callers wait at Recv; async callers use an exact reply token.
    let _ = pending.push(PendingDns {
        owner, sender, op_id: ostd::ipc::current(), query: Some(query),
        answer: None, deadline: sys_get_time().saturating_add(DNS_QUERY_DEADLINE_TICKS),
    });
    true
}

fn poll_resolves(
    resolver: &Resolver,
    pending: &mut heapless::Vec<PendingDns, DNS_QUERY_LIMIT>,
    sockets: &mut SocketSet<'_>,
) {
    let now = sys_get_time();
    let mut index = 0;
    while index < pending.len() {
        let state = &mut pending[index];
        if state.answer.is_none() {
            if let Some(query) = state.query {
                if let Some(result) = resolver.poll_result(query, sockets) {
                    state.query = None;
                    state.answer = Some(result);
                } else if now >= state.deadline {
                    resolver.cancel(query, sockets);
                    state.query = None;
                    state.answer = Some(None);
                }
            }
        }
        let Some(answer) = state.answer else {
            index += 1;
            continue;
        };
        let mut out = [0u8; IPC_BUF_SIZE];
        let response = match answer {
            Some(ip) => NetResponse::Addr(ip),
            None => NetResponse::Err(0xFF),
        };
        let sent = ipc::encode(&response, &mut out).is_ok_and(|bytes| match state.op_id {
            Some(op_id) => {
                let _ = ostd::ipc::reply(op_id, bytes);
                true // A cancelled op cannot be delivered to another caller.
            }
            None => matches!(ostd::syscall::sys_try_send(state.sender, bytes), SyscallResult::Ok(0)),
        });
        if sent || now >= state.deadline.saturating_add(10_000_000) {
            pending.swap_remove(index);
        } else {
            index += 1;
        }
    }
}

const WAITSET_LIMIT: usize = 16;
const WAITSET_DEADLINE_TICKS: u64 = 50_000_000;

struct PendingReady {
    op_id: usize,
    interests: heapless::Vec<u8, {MAX_INTERESTS * ipc::NET_READY_RECORD_BYTES}>,
    cursor: u16,
    deadline: u64,
}

fn send_ready(sender: usize, response: NetResponse<'_>) {
    let mut buf = [0u8; IPC_BUF_SIZE];
    if let Ok(bytes) = ipc::encode(&response, &mut buf) {
        let _ = ostd::syscall::sys_send(sender, bytes);
    }
}

/// One deferred IPC token represents up to 256 socket interests. Register only
/// after authorization; recheck before parking and again on every net turn.
fn handle_ready_wait(
    buffer: &[u8],
    sender: usize,
    owner: SocketOwner,
    sockets: &mut SocketSet<'_>,
    table: &SocketTable,
    pending: &mut BTreeMap<SocketOwner, PendingReady>,
) -> bool {
    let Ok(NetRequest::TcpReady { interests, cursor, wait: true }) =
        ipc::decode::<NetRequest<'_>>(buffer)
    else {
        return false;
    };
    let result = readiness(interests, cursor, owner, sockets, table);
    match result {
        Err(()) => send_ready(sender, NetResponse::Err(0xFF)),
        Ok((events, next_cursor)) if !events.is_empty() => {
            let mut packed = [0u8; handlers::tcp::READY_PAGE * ipc::NET_READY_RECORD_BYTES];
            send_ready(sender, NetResponse::TcpReady {
                events: handlers::tcp::pack_events(&events, &mut packed), next_cursor,
            });
        }
        Ok(_) => {
            let Some(op_id) = ostd::ipc::current() else {
                send_ready(sender, NetResponse::Err(0xFD));
                return true;
            };
            if let Some(previous) = pending.remove(&owner) {
                let mut out = [0u8; IPC_BUF_SIZE];
                if let Ok(bytes) = ipc::encode(&NetResponse::Err(0xFD), &mut out) {
                    let _ = ostd::ipc::reply(previous.op_id, bytes);
                }
            }
            if pending.len() >= WAITSET_LIMIT {
                send_ready(sender, NetResponse::Err(0xFD));
                return true;
            }
            let mut owned = heapless::Vec::new();
            for &byte in interests {
                if owned.push(byte).is_err() {
                    send_ready(sender, NetResponse::Err(0xFF));
                    return true;
                }
            }
            pending.insert(owner, PendingReady {
                op_id,
                interests: owned,
                cursor,
                deadline: sys_get_time().saturating_add(WAITSET_DEADLINE_TICKS),
            });
        }
    }
    true
}

/// FIN queues output before the close. Closed completes normally; the finite
/// deadline is an explicit abort of a peer that never acknowledges or closes.
fn reap_graceful_closes(table: &mut SocketTable, sockets: &mut SocketSet<'_>) {
    let now = sys_get_time();
    for _ in 0..8 {
        let next = table.graceful_closes().find_map(|(cap, deadline)| {
            let done = table
                .get_unchecked(cap)
                .is_none_or(|h| sockets.get_mut::<smoltcp::socket::tcp::Socket>(h).state()
                    == smoltcp::socket::tcp::State::Closed);
            (done || now >= deadline).then_some(cap)
        });
        let Some(cap) = next else { break };
        if let Some(h) = table.remove_internal(cap) {
            sockets.remove(h);
        }
    }
}

fn drain_ready_waits(
    pending: &mut BTreeMap<SocketOwner, PendingReady>,
    sockets: &mut SocketSet<'_>,
    table: &SocketTable,
) {
    let now = sys_get_time();
    // Bound the owner scan: registration admits at most WAITSET_LIMIT records.
    let mut completed = heapless::Vec::<SocketOwner, WAITSET_LIMIT>::new();
    for (&owner, wait) in pending.iter() {
        let result = readiness(&wait.interests, wait.cursor, owner, sockets, table);
        let response = match result {
            Err(()) => Some(NetResponse::Err(0xFF)),
            Ok((events, next_cursor)) if !events.is_empty() => {
                let mut packed = [0u8; handlers::tcp::READY_PAGE * ipc::NET_READY_RECORD_BYTES];
                let mut out = [0u8; IPC_BUF_SIZE];
                let response = NetResponse::TcpReady {
                    events: handlers::tcp::pack_events(&events, &mut packed), next_cursor,
                };
                if let Ok(bytes) = ipc::encode(&response, &mut out) {
                    let _ = ostd::ipc::reply(wait.op_id, bytes);
                }
                let _ = completed.push(owner);
                continue;
            }
            Ok(_) if now >= wait.deadline => Some(NetResponse::NotReady),
            Ok(_) => None,
        };
        if let Some(response) = response {
            let mut out = [0u8; IPC_BUF_SIZE];
            if let Ok(bytes) = ipc::encode(&response, &mut out) {
                let _ = ostd::ipc::reply(wait.op_id, bytes);
            }
            let _ = completed.push(owner);
        }
    }
    for owner in completed {
        pending.remove(&owner);
    }
}
fn release_owner(
    owner: SocketOwner,
    sockets: &mut SocketSet<'_>,
    table: &mut SocketTable,
    tls_table: &mut BTreeMap<u64, TlsSocketEntry>,
    tls_pending: &mut crate::tls_handler::TlsPending,
    pending_ready: &mut BTreeMap<SocketOwner, PendingReady>,
    pending_dns: &mut heapless::Vec<PendingDns, DNS_QUERY_LIMIT>,
    resolver: &Resolver,
    watched: &mut BTreeMap<SocketOwner, (usize, u64)>,
) {
    pending_ready.remove(&owner);
    let mut index = 0;
    while index < pending_dns.len() {
        if pending_dns[index].owner == owner {
            if let Some(query) = pending_dns[index].query {
                resolver.cancel(query, sockets);
            }
            pending_dns.swap_remove(index);
        } else {
            index += 1;
        }
    }
    tls_pending.cancel_owner(owner, sockets, table);
    while let Some(cap) = table.owned_cap(owner) {
        tls_table.remove(&cap);
        if let Some(handle) = table.remove_internal(cap) {
            sockets.remove(handle);
        }
    }
    if let Some((_, token)) = watched.remove(&owner) {
        ostd::syscall::sys_cancel_cell_owner_watch(token);
    }
}


#[cfg(target_os = "none")]
pub(crate) fn run() {
    println("[net] Network Service v0.1: smoltcp + NIC Driver Cell + DHCP");

    let mut device = VirtioNetDevice::new();
    let config = Config::new(HardwareAddress::Ethernet(MAC));
    let mut iface = Interface::new(config, &mut device, now_instant());
    iface.update_ip_addrs(|addresses| {
        let _ = addresses.push(IpCidr::new(IpAddress::v4(0, 0, 0, 0), 0));
    });

    // Allocate slots incrementally: the 290-cap profile cannot hold the
    // complete socket table on its small cell stack.
    let mut socket_storage = alloc::vec::Vec::with_capacity(SOCKET_SET_STORAGE);
    socket_storage.resize_with(SOCKET_SET_STORAGE, || SocketStorage::EMPTY);
    let mut sockets = SocketSet::new(&mut socket_storage[..]);
    let mut table = SocketTable::new();
    let mut tls_table: BTreeMap<u64, TlsSocketEntry> = BTreeMap::new();
    let mut tls_pending = crate::tls_handler::TlsPending::default();
    let mut pending_ready: BTreeMap<SocketOwner, PendingReady> = BTreeMap::new();
    let mut pending_dns = heapless::Vec::<PendingDns, DNS_QUERY_LIMIT>::new();
    // Root TID and death-watch token. Worker TID exit is not root death.
    let mut watched: BTreeMap<SocketOwner, (usize, u64)> = BTreeMap::new();
    // One DNS socket serves every `NetRequest::Resolve`; the server starts as
    // SLIRP's and is replaced by whatever the DHCP lease names.
    let mut resolver = Resolver::install(&mut sockets);

    #[cfg(not(feature = "hypervisor-bridge"))]
    let dhcp_handle = add_dhcp_socket(&mut sockets);
    #[cfg(not(feature = "hypervisor-bridge"))]
    let mut dhcp_state = DhcpState::Pending;

    let mut buffer = [0u8; IPC_BUF_SIZE];
    #[cfg(not(feature = "hypervisor-bridge"))]
    let mut last_poll_ticks = sys_get_time();
    let mut local_ip = [0u8; 4];
    #[cfg(not(feature = "hypervisor-bridge"))]
    let mut net_rx_producer_proved = false;
    #[cfg(not(feature = "hypervisor-bridge"))]
    let mut pending_net_rx_proof = false;
    #[cfg(not(feature = "hypervisor-bridge"))]
    let mut ipc_burst_grace = 0;
    #[cfg(all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge")))]
    let mut idle_ipc_wake_oracle = crate::idle_ipc_wake_oracle::IdleIpcWakeOracle::new();

    #[cfg(not(feature = "hypervisor-bridge"))]
    println("[net] Starting DHCP...");

    #[cfg(feature = "loop-trace")]
    let mut trace_turns: u64 = 0;
    #[cfg(feature = "loop-trace")]
    let mut trace_recv: u64 = 0;
    #[cfg(feature = "loop-trace")]
    let mut trace_last = sys_get_time();

    loop {
        ostd::syscall::sys_heartbeat(500);
        // `loop-trace` image: one line every ~2 s with what the turn is spending
        // its time on. A stopped heartbeat is the signal that the service is
        // parked inside a driver command rather than looping.
        #[cfg(feature = "loop-trace")]
        {
            trace_turns += 1;
            let now = sys_get_time();
            if now.wrapping_sub(trace_last) >= LOOP_TRACE_INTERVAL_TICKS {
                trace_last = now;
                println(&alloc::format!(
                    "[net-loop] turns={} recv={} l2send={} l2recv={} drv_cmd={} drv_to={} rx_q={} guest_q={}",
                    trace_turns,
                    trace_recv,
                    crate::handlers::L2_SEND_REQUESTS.load(Ordering::Relaxed),
                    crate::handlers::L2_RECV_REQUESTS.load(Ordering::Relaxed),
                    crate::interface::DRV_COMMANDS.load(Ordering::Relaxed),
                    crate::interface::DRV_TIMEOUTS.load(Ordering::Relaxed),
                    device.rx_queue_len(),
                    device.guest_rx_queue_len(),
                ));
            }
        }
        #[cfg(not(feature = "hypervisor-bridge"))]
        {
            let drained = device.pump_rx_split();
            if pending_net_rx_proof {
                if !net_rx_producer_proved && drained > 0 {
                    println("[net-rx-producer] irq->completion PASS");
                    net_rx_producer_proved = true;
                }
                pending_net_rx_proof = false;
            }

            if dhcp_state == DhcpState::Pending {
                let lease = poll_dhcp(
                    dhcp_handle,
                    &mut iface,
                    &mut sockets,
                    &mut device,
                    now_instant(),
                );
                dhcp_state = lease.state;
                if dhcp_state == DhcpState::Acquired {
                    if let Some(IpCidr::Ipv4(cidr)) = iface
                        .ip_addrs()
                        .iter()
                        .find(|address| matches!(address, IpCidr::Ipv4(_)))
                    {
                        local_ip.copy_from_slice(cidr.address().as_bytes());
                        println(&alloc::format!(
                            "[net] IP address: {}",
                            crate::service_runtime::dotted_ipv4(local_ip)
                        ));
                    }
                }
                // The lease names the resolver to use from here on (option 6);
                // without it the DNS socket keeps SLIRP's built-in default.
                if let Some(server) = lease.dns_server {
                    resolver.set_server(&mut sockets, server);
                    println(&alloc::format!(
                        "[net] DNS server: {}",
                        crate::service_runtime::dotted_ipv4(server)
                    ));
                }
            }

            let now = sys_get_time();
            // Poll on every turn, not only on the 100 ms maintenance cadence:
            // smoltcp advances a handshake one step per poll, and a listener
            // stuck in SynReceived cannot report ACCEPT. Waiting for the
            // maintenance tick added up to 100 ms per handshake step.
            iface.poll(now_instant(), &mut device, &mut sockets);
            if now.wrapping_sub(last_poll_ticks) >= SMOLTCP_MAINTENANCE_TICKS {
                last_poll_ticks = now;
            }
        }
        tls_pending.poll(&mut iface, &mut device, &mut sockets, &mut table, &mut tls_table);
        poll_resolves(&resolver, &mut pending_dns, &mut sockets);
        reap_graceful_closes(&mut table, &mut sockets);
        drain_ready_waits(&mut pending_ready, &mut sockets, &table);

        buffer.fill(0);
        let receive_result = sys_try_recv_attested(0, &mut buffer);
        match receive_result {
            SyscallResult::Ok(sender) if sender > 0 => {
                #[cfg(feature = "loop-trace")]
                {
                    trace_recv += 1;
                }
                let Some(identity) = api::caller_identity::CallerIdentity::from_recv_buf(&buffer)
                else {
                    if let Some(owner) = watched.iter().find_map(|(&owner, &(root_tid, _))|
                        (root_tid == sender).then_some(owner))
                    {
                        release_owner(owner, &mut sockets, &mut table, &mut tls_table,
                            &mut tls_pending, &mut pending_ready, &mut pending_dns, &resolver,
                            &mut watched);
                    }
                    #[cfg(all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge")))]
                    idle_ipc_wake_oracle.record_ipc_miss();
                    continue;
                };
                if identity.cell_id == 0 || identity.generation == 0 {
                    #[cfg(all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge")))]
                    idle_ipc_wake_oracle.record_ipc_miss();
                    continue;
                }
                let owner = SocketOwner {
                    cell_id: identity.cell_id,
                    generation: identity.generation,
                };
                // Owner watch is a cleanup optimisation: without it a dead
                // client's sockets are only reclaimed when the net cell
                // restarts, so a refusal must never deny live traffic.
                if !watched.contains_key(&owner) {
                    let watch = ostd::syscall::sys_watch_cell_owner(owner.cell_id, owner.generation);
                    if let Some((principal, token)) = watch {
                        while let Some(stale) = watched.keys().copied().find(|candidate|
                            candidate.cell_id == owner.cell_id && *candidate != owner)
                        {
                            release_owner(stale, &mut sockets, &mut table, &mut tls_table,
                                &mut tls_pending, &mut pending_ready, &mut pending_dns, &resolver,
                                &mut watched);
                        }
                        watched.insert(owner, (principal.root_tid as usize, token));
                    } else if !WATCH_REFUSED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                        println("[net] owner death watch unavailable; socket reclaim deferred");
                    }
                }
                #[cfg(all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge")))]
                idle_ipc_wake_oracle.record_ipc_drain(sys_get_time(), sender);
                // A ready wait is registered only after a network progress
                // pass. The retained interest batch is rechecked before idle
                // sleep and on every NET_RX / maintenance wake.
                iface.poll(now_instant(), &mut device, &mut sockets);
                if !handle_ready_wait(&buffer, sender, owner, &mut sockets, &table, &mut pending_ready)
                    && !handle_resolve(&buffer, sender, owner, &mut iface, &mut sockets,
                        &resolver, &mut pending_dns)
                {
                    handlers::handle_request(
                        &buffer, sender, owner, &mut iface, &mut device, &mut sockets,
                        &mut table, &mut tls_table, &mut tls_pending, &resolver, &local_ip,
                    );
                }
                drain_ready_waits(&mut pending_ready, &mut sockets, &table);
                #[cfg(not(feature = "hypervisor-bridge"))]
                {
                    ipc_burst_grace = IPC_BURST_GRACE_YIELDS;
                }
            }
            _ => {
                #[cfg(all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge")))]
                idle_ipc_wake_oracle.record_ipc_miss();
                #[cfg(feature = "hypervisor-bridge")]
                {
                    let _ = sys_wait_completion(NET_RX, 1);
                }
                #[cfg(not(feature = "hypervisor-bridge"))]
                if consume_ipc_burst_grace(&mut ipc_burst_grace) {
                    // Let the caller run after a reply before parking on NET_RX;
                    // sequential IPC bursts then avoid one timer quantum per call.
                    sys_yield();
                } else {
                    #[cfg(feature = "ipc-wake-oracle")]
                    let wait_started_ticks = sys_get_time();
                    #[cfg(feature = "ipc-wake-oracle")]
                    let completion = match sys_wait_completion_detailed(
                        NET_RX,
                        NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS,
                    ) {
                        WaitCompletionResult::NoRecord => {
                            idle_ipc_wake_oracle.arm(
                                wait_started_ticks,
                                sys_get_time().wrapping_sub(wait_started_ticks),
                                SMOLTCP_MAINTENANCE_TICKS,
                                IDLE_IPC_WAKE_PROOF_CEILING_TICKS,
                            );
                            None
                        }
                        WaitCompletionResult::Completion(completion) => {
                            idle_ipc_wake_oracle.clear();
                            Some(completion)
                        }
                        WaitCompletionResult::ErrorOrInvalid(_) => {
                            idle_ipc_wake_oracle.clear();
                            None
                        }
                    };
                    #[cfg(not(feature = "ipc-wake-oracle"))]
                    let completion =
                        sys_wait_completion(NET_RX, NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS);
                    if let Some(completion) = completion {
                        pending_net_rx_proof =
                            completion.source == NET_RX && completion.result == NET_RX as i64;
                    }
                }
                // A recordless return means queued IPC interrupted the wait early or
                // the finite maintenance budget elapsed. Either way, retry the loop
                // without claiming NET_RX producer proof.
            }
        }
    }
}

#[cfg(test)]
#[path = "service-runtime-tests.rs"]
mod tests;
