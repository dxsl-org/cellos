use alloc::{collections::BTreeMap, string::String};
use core::sync::atomic::Ordering;
use ostd::service::Owner;
use ostd::syscall::SyscallResult;
use smoltcp::{
    iface::{Interface, SocketSet},
    time::Instant,
    wire::{EthernetAddress, IpAddress, IpCidr},
};
use wire::ipc::{self, NetRequest, NetResponse};
use wire::proto::net::NetError;
#[cfg(target_os = "none")]
use {
    alloc::vec::Vec,
    net_state::NetState,
    ostd::io::println,
    ostd::service::owners::KernelWatch,
    ostd::service::{serve, Handler, Request, ServeConfig, Wait},
    ostd::syscall::sys_yield,
    smoltcp::iface::SocketStorage,
};

use crate::{
    dns::Resolver,
    handlers,
    handlers::tcp::{readiness, MAX_INTERESTS},
    interface::VirtioNetDevice,
    socket_table::{SocketOwner, SocketTable},
    tls::socket::TlsSocketEntry,
};

#[cfg(target_os = "none")]
#[path = "net_pump.rs"]
mod net_pump;
#[cfg(target_os = "none")]
#[path = "net_state.rs"]
mod net_state;
#[cfg(any(test, target_os = "none"))]
#[path = "net-rx-proof.rs"]
mod net_rx_proof;
#[cfg(target_os = "none")]
#[path = "probe.rs"]
mod probe;

const IPC_BUF_SIZE: usize = 4096;
const MAC: EthernetAddress = EthernetAddress([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
#[allow(dead_code)]
const NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS: u64 = 10;
#[allow(dead_code)]
const SCHEDULER_TICK_MS: u64 = 10;
#[allow(dead_code)]
const IPC_BURST_GRACE_YIELDS: u8 = 1;
#[allow(dead_code)]
const SMOLTCP_MAINTENANCE_INTERVAL_MS: u64 = 100;
const MTIME_TICKS_PER_MS: u64 = 10_000;
#[allow(dead_code)]
const SMOLTCP_MAINTENANCE_TICKS: u64 = SMOLTCP_MAINTENANCE_INTERVAL_MS * MTIME_TICKS_PER_MS;
/// How long a socket aborted on owner death may wait for its RST to leave
/// (nominally 1 s at the assumed 10 MHz tick rate, like the graceful-close
/// deadline). Only a fallback for a TX path that cannot take a frame.
const ABORT_DRAIN_TICKS: u64 = 1_000 * MTIME_TICKS_PER_MS;
/// Aborted sockets finished per loop turn.
const ABORT_REAP_PER_TURN: usize = 32;
// A 10-tick deadline can fire just after 9 complete tick periods when the
// submission lands immediately before a scheduler tick. The exclusive ceiling
// therefore has to be the preceding 9 periods, not the 100 ms maintenance budget.
#[cfg(any(test, feature = "ipc-wake-oracle"))]
const IDLE_IPC_WAKE_PROOF_CEILING_TICKS: u64 =
    (NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS - 1) * SCHEDULER_TICK_MS * MTIME_TICKS_PER_MS;

#[cfg(any(test, feature = "ipc-wake-oracle"))]
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
#[cfg(any(test, feature = "ipc-wake-oracle"))]
pub(crate) const fn classify_idle_ipc_wake(wait_return_ticks: u64) -> IdleIpcWakeClassification {
    if wait_return_ticks < IDLE_IPC_WAKE_PROOF_CEILING_TICKS {
        IdleIpcWakeClassification::Pass
    } else {
        IdleIpcWakeClassification::Inconclusive
    }
}
/// One-shot log guard for a refused owner death watch.
#[cfg(target_os = "none")]
static WATCH_REFUSED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// Owners watched at once: no cap of its own, like the private map this table
/// replaced. A refused watch only defers reclaim to a net restart.
const OWNER_WATCH_CAPACITY: usize = usize::MAX;

/// Normalize architecture-specific raw clocks to the NET 10 MHz timebase.
pub(crate) fn sys_get_time() -> u64 {
    ostd::syscall::sys_get_time_ms()
        .unwrap_or(u64::MAX / 10_000)
        .saturating_mul(10_000)
}

pub(crate) fn now_instant() -> Instant {
    Instant::from_micros((sys_get_time() / 10) as i64)
}

/// Dotted-quad text for log lines (`10.0.2.15`).
#[allow(dead_code)]
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

#[allow(dead_code)]
fn consume_ipc_burst_grace(remaining: &mut u8) -> bool {
    if *remaining == 0 {
        return false;
    }
    *remaining -= 1;
    true
}

const DNS_QUERY_LIMIT: usize = 32;
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
        send_ready(sender, NetResponse::Err(NetError::Busy as u8));
        return true;
    }
    let Some(query) = resolver.start(hostname, iface, sockets) else {
        send_ready(sender, NetResponse::Err(NetError::Failure as u8));
        return true;
    };
    // Legacy callers wait at Recv; async callers use an exact reply token.
    let _ = pending.push(PendingDns {
        owner,
        sender,
        op_id: ostd::ipc::current(),
        query: Some(query),
        answer: None,
        deadline: sys_get_time().saturating_add(DNS_QUERY_DEADLINE_TICKS),
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
            None => NetResponse::Err(NetError::Failure as u8),
        };
        let sent = ipc::encode(&response, &mut out).is_ok_and(|bytes| match state.op_id {
            Some(op_id) => {
                let _ = ostd::ipc::reply(op_id, bytes);
                true // A cancelled op cannot be delivered to another caller.
            }
            None => matches!(
                ostd::syscall::sys_try_send(state.sender, bytes),
                SyscallResult::Ok(0)
            ),
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
    interests: heapless::Vec<u8, { MAX_INTERESTS * ipc::NET_READY_RECORD_BYTES }>,
    cursor: u16,
    deadline: u64,
}

fn send_ready(sender: usize, response: NetResponse<'_>) {
    let mut buf = [0u8; IPC_BUF_SIZE];
    if let Ok(bytes) = ipc::encode(&response, &mut buf) {
        let _ = ostd::ipc::reply_to(sender, bytes);
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
    let Ok(NetRequest::TcpReady {
        interests,
        cursor,
        wait: true,
    }) = ipc::decode::<NetRequest<'_>>(buffer)
    else {
        return false;
    };
    let result = readiness(interests, cursor, owner, sockets, table);
    match result {
        Err(()) => send_ready(sender, NetResponse::Err(NetError::Failure as u8)),
        Ok((events, next_cursor)) if !events.is_empty() => {
            let mut packed = [0u8; handlers::tcp::READY_PAGE * ipc::NET_READY_RECORD_BYTES];
            send_ready(
                sender,
                NetResponse::TcpReady {
                    events: handlers::tcp::pack_events(&events, &mut packed),
                    next_cursor,
                },
            );
        }
        Ok(_) => {
            let Some(op_id) = ostd::ipc::current() else {
                send_ready(sender, NetResponse::Err(NetError::Busy as u8));
                return true;
            };
            if let Some(previous) = pending.remove(&owner) {
                let mut out = [0u8; IPC_BUF_SIZE];
                if let Ok(bytes) = ipc::encode(&NetResponse::Err(NetError::Busy as u8), &mut out) {
                    let _ = ostd::ipc::reply(previous.op_id, bytes);
                }
            }
            if pending.len() >= WAITSET_LIMIT {
                send_ready(sender, NetResponse::Err(NetError::Busy as u8));
                return true;
            }
            let mut owned = heapless::Vec::new();
            for &byte in interests {
                if owned.push(byte).is_err() {
                    send_ready(sender, NetResponse::Err(NetError::Failure as u8));
                    return true;
                }
            }
            pending.insert(
                owner,
                PendingReady {
                    op_id,
                    interests: owned,
                    cursor,
                    deadline: sys_get_time().saturating_add(WAITSET_DEADLINE_TICKS),
                },
            );
        }
    }
    true
}

/// What a graceful-close ledger entry needs from the reaper this turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GracefulEnd {
    /// Nothing is owed on the wire: free the slot.
    Free,
    /// The peer must be told: `abort()` (one RST), drain through the abort ledger.
    Abort,
    /// Still closing and inside its deadline.
    Keep,
}

fn graceful_end(state: smoltcp::socket::tcp::State, rst_owed: bool, expired: bool) -> GracefulEnd {
    use smoltcp::socket::tcp::State;
    match state {
        State::Closed if rst_owed => GracefulEnd::Abort, // smoltcp already aborted it and still owes the RST
        State::Closed => GracefulEnd::Free, // peer FIN/ACK done, peer RST, or never connected
        State::TimeWait if expired => GracefulEnd::Free, // both FINs exchanged and acked: a clean end
        State::TimeWait => GracefulEnd::Keep,            // smoltcp's own 10 s timer finishes it
        _ if expired => GracefulEnd::Abort, // FinWait1/FinWait2/Closing/LastAck (and any state not expected after close())
        _ => GracefulEnd::Keep,
    }
}

/// Graceful closes finished per loop turn.
const GRACEFUL_REAP_PER_TURN: usize = 8;

/// FIN queues output before the close. A finished close frees the slot; the finite
/// deadline is an explicit abort of a peer that never acknowledges or closes: one RST,
/// then the abort ledger waits until it has left. `now` is passed in so the deadline is
/// testable. Returns how many sockets were aborted this turn.
fn reap_graceful_closes(table: &mut SocketTable, sockets: &mut SocketSet<'_>, now: u64) -> usize {
    let mut aborted = 0;
    for _ in 0..GRACEFUL_REAP_PER_TURN {
        let next = table.graceful_closes().find_map(|(cap, deadline)| {
            let end = match table.get_unchecked(cap) {
                None => GracefulEnd::Free,
                Some(handle) => {
                    let socket = sockets.get::<smoltcp::socket::tcp::Socket>(handle);
                    graceful_end(
                        socket.state(),
                        socket.remote_endpoint().is_some(),
                        now >= deadline,
                    )
                }
            };
            (end != GracefulEnd::Keep).then_some((cap, end))
        });
        let Some((cap, end)) = next else { break };
        if end == GracefulEnd::Abort {
            if let Some(handle) = table.get_unchecked(cap) {
                let socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
                socket.abort();
                if socket.remote_endpoint().is_some() {
                    table.graceful_to_abort(cap, now.saturating_add(ABORT_DRAIN_TICKS));
                    aborted += 1;
                    continue;
                }
            }
        }
        if let Some(handle) = table.remove_internal(cap) {
            sockets.remove(handle);
        }
    }
    aborted
}

/// One line per reaper turn that aborted something (normal clients finish inside the
/// deadline, so it appears only for stuck peers; same rule as the owner-death line).
fn graceful_deadline_line(aborted: usize) -> Option<String> {
    (aborted > 0).then(|| alloc::format!("[net] graceful close deadline: aborted {aborted} TCP"))
}

/// An aborted socket is finished once its RST has left (smoltcp clears the
/// connection tuple when it emits it) or, as a fallback, at its deadline.
/// `now` is passed in so the deadline is testable.
fn reap_aborted(table: &mut SocketTable, sockets: &mut SocketSet<'_>, now: u64) {
    for _ in 0..ABORT_REAP_PER_TURN {
        let next = table.aborting().find_map(|(cap, deadline)| {
            let sent = table.get_unchecked(cap).is_none_or(|h| {
                sockets
                    .get::<smoltcp::socket::tcp::Socket>(h)
                    .remote_endpoint()
                    .is_none()
            });
            (sent || now >= deadline).then_some(cap)
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
            Err(()) => Some(NetResponse::Err(NetError::Failure as u8)),
            Ok((events, next_cursor)) if !events.is_empty() => {
                let mut packed = [0u8; handlers::tcp::READY_PAGE * ipc::NET_READY_RECORD_BYTES];
                let mut out = [0u8; IPC_BUF_SIZE];
                let response = NetResponse::TcpReady {
                    events: handlers::tcp::pack_events(&events, &mut packed),
                    next_cursor,
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

/// Gate one attested request on the caller's network rights.
///
/// A denial is answered here; the caller then skips the owner watch, both
/// intercepts and dispatch, so the request leaves no socket, DNS query, owner
/// watch or frame behind.
fn admit_request(buffer: &[u8], sender: usize, flags: u32) -> bool {
    let Err(denial) = crate::rights::admit(buffer, flags) else {
        return true;
    };
    let mut reply = [0u8; crate::rights::DENIAL_REPLY_LEN];
    let _ = ostd::ipc::reply_to(sender, denial.reply(&mut reply));
    false
}

/// Serve one admitted request: the ready-wait and resolve intercepts, then
/// typed or raw TLS dispatch.
#[allow(clippy::too_many_arguments)]
fn dispatch_admitted(
    buffer: &[u8],
    sender: usize,
    owner: SocketOwner,
    iface: &mut Interface,
    device: &mut VirtioNetDevice,
    sockets: &mut SocketSet<'_>,
    table: &mut SocketTable,
    tls_table: &mut BTreeMap<u64, TlsSocketEntry>,
    tls_pending: &mut crate::tls_handler::TlsPending,
    pending_ready: &mut BTreeMap<SocketOwner, PendingReady>,
    pending_dns: &mut heapless::Vec<PendingDns, DNS_QUERY_LIMIT>,
    resolver: &Resolver,
    local_ip: &[u8; 4],
) {
    // A ready wait is registered only after a network progress
    // pass. The retained interest batch is rechecked before idle
    // sleep and on every NET_RX / maintenance wake.
    iface.poll(now_instant(), device, sockets);
    if !handle_ready_wait(buffer, sender, owner, sockets, table, pending_ready)
        && !handle_resolve(buffer, sender, owner, iface, sockets, resolver, pending_dns)
    {
        handlers::handle_request(
            buffer,
            sender,
            owner,
            iface,
            device,
            sockets,
            table,
            tls_table,
            tls_pending,
            resolver,
            local_ip,
        );
    }
}

/// The socket owner a watched [`Owner`] names.
const fn socket_owner(owner: Owner) -> SocketOwner {
    SocketOwner {
        cell_id: owner.cell_id,
        generation: owner.generation,
    }
}

/// Everything a dead owner generation holds in the service, without kernel
/// calls: ready wait and DNS queries dropped (no reply is owed: the kernel
/// cancelled the dead owner's async operations), in-flight TLS dropped,
/// established-TLS state dropped (no close-notify: a dead cell cannot answer),
/// then every remaining socket aborted or freed. Returns `(aborted_tcp,
/// released_other)` from [`SocketTable::abort_owner`].
#[allow(clippy::too_many_arguments)]
fn reclaim_owner_state(
    owner: SocketOwner,
    sockets: &mut SocketSet<'_>,
    table: &mut SocketTable,
    tls_table: &mut BTreeMap<u64, TlsSocketEntry>,
    tls_pending: &mut crate::tls_handler::TlsPending,
    pending_ready: &mut BTreeMap<SocketOwner, PendingReady>,
    pending_dns: &mut heapless::Vec<PendingDns, DNS_QUERY_LIMIT>,
    resolver: &Resolver,
    abort_deadline: u64,
) -> (usize, usize) {
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
    tls_table.retain(|cap, _| !table.is_owner(*cap, owner));
    table.abort_owner(owner, sockets, abort_deadline)
}

/// The one log line of an owner death, only when something was reclaimed
/// (normal clients close their sockets first, so it appears exactly for leaks).
fn owner_death_line(owner: SocketOwner, aborted: usize, released: usize) -> Option<String> {
    (aborted + released > 0).then(|| {
        alloc::format!(
            "[net] owner death: cell {} gen {}: aborted {} TCP, released {} other",
            owner.cell_id,
            owner.generation,
            aborted,
            released
        )
    })
}

/// Replace the interface's IPv4 address (DHCP and link-local share one slot).
#[cfg(target_os = "none")]
fn set_ipv4(iface: &mut Interface, cidr: IpCidr) {
    iface.update_ip_addrs(|addrs| {
        if let Some(slot) = addrs.iter_mut().find(|a| matches!(a, IpCidr::Ipv4(_))) {
            *slot = cidr;
        } else {
            let _ = addrs.push(cidr);
        }
    });
}

/// Apply one RFC 3927 engine step. The address is configured at the claim so
/// smoltcp answers ARP for it while the announcements go out; `local_ip` (what
/// `ifconfig` reports) follows at the bind.
#[cfg(target_os = "none")]
fn apply_link_local(
    event: crate::link_local::Event,
    iface: &mut Interface,
    device: &mut VirtioNetDevice,
    local_ip: &mut [u8; 4],
) {
    use crate::arp::mac_text;
    use crate::link_local::{Event, PeerKind};
    let line = match event {
        Event::Send(packet) => {
            // A full TX queue loses this frame only: probes and announcements
            // repeat, and a rival that keeps claiming triggers the next defense.
            let _ = device.send_frame(&packet.encode_frame());
            return;
        }
        Event::Probing { ip, attempt } => alloc::format!(
            "[net] link-local: probing {} (candidate {attempt})",
            dotted_ipv4(ip)
        ),
        Event::Claim(ip) => {
            let [a, b, c, d] = ip;
            set_ipv4(iface, IpCidr::new(IpAddress::v4(a, b, c, d), 16));
            alloc::format!("[net] link-local: announcing {}", dotted_ipv4(ip))
        }
        Event::Bound(ip) => {
            *local_ip = ip;
            alloc::format!(
                "[net] IP address (IPv4 Link-Local RFC 3927): {}",
                dotted_ipv4(ip)
            )
        }
        Event::Conflict { ip, mac } => alloc::format!(
            "[net] link-local: conflict on {} with {}; choosing another address",
            dotted_ipv4(ip),
            mac_text(mac)
        ),
        Event::Defended { ip, mac } => alloc::format!(
            "[net] link-local: defended {} against {}",
            dotted_ipv4(ip),
            mac_text(mac)
        ),
        Event::Release { ip, mac } => {
            set_ipv4(iface, IpCidr::new(IpAddress::v4(0, 0, 0, 0), 0));
            *local_ip = [0; 4];
            alloc::format!(
                "[net] link-local: relinquished {} to {}",
                dotted_ipv4(ip),
                mac_text(mac)
            )
        }
        Event::Peer { ip, mac, kind } => alloc::format!(
            "[net] link-local: peer {} at {} ({})",
            dotted_ipv4(ip),
            mac_text(mac),
            match kind {
                PeerKind::Announce => "announce",
                PeerKind::Request => "request",
                PeerKind::Reply => "reply",
            }
        ),
    };
    println(&line);
}

#[cfg(target_os = "none")]
pub(crate) fn run() -> ! {
    // Allocate slots incrementally: the 290-cap profile cannot hold the
    // complete socket table on its small cell stack. The socket set borrows
    // this storage; `serve` never returns, so this frame owns it for the life
    // of the service and `NetState` needs neither a leak nor a self-reference.
    let mut socket_storage = Vec::with_capacity(crate::socket_table::SOCKET_SET_STORAGE);
    socket_storage.resize_with(crate::socket_table::SOCKET_SET_STORAGE, || {
        SocketStorage::EMPTY
    });
    let mut state = NetState::new(&mut socket_storage[..]);
    let config = ServeConfig {
        heartbeat_ticks: 500,
        ..ServeConfig::with_heartbeat()
    };
    // Owner watches stay in the handler: admission precedes every effect of a
    // request, the watch included, so a denied caller leaves no state behind.
    serve(config, &mut state)
}

/// One turn: [`NetState::pump`], then one attested request, an owner death,
/// or a park on NET_RX.
#[cfg(target_os = "none")]
impl Handler for NetState<'_> {
    fn on_idle(&mut self) -> Wait {
        self.pump();
        Wait::Poll
    }

    fn on_request(&mut self, request: Request<'_>) {
        self.probe.received();
        let caller = *request.caller();
        if caller.cell_id == 0 || caller.generation == 0 {
            self.probe.ipc_miss();
            return;
        }
        let (buffer, sender) = (request.payload(), request.sender());
        // Admission precedes every effect of the request, the owner
        // watch included: a denied caller leaves no state behind.
        if !admit_request(buffer, sender, caller.flags) {
            self.probe.ipc_drain(sender);
            self.ipc_burst_grace = IPC_BURST_GRACE_YIELDS;
            return;
        }
        // Owner watch is a cleanup optimisation: without it a dead client's
        // sockets are only reclaimed when the net cell restarts, so a refusal
        // must never deny live traffic.
        let admission = self.watches.admit(&caller, &mut KernelWatch);
        if !admission.watched && !WATCH_REFUSED.swap(true, Ordering::Relaxed) {
            println("[net] owner death watch unavailable; socket reclaim deferred");
        }
        // The caller's Cell was relaunched: its older generations are gone.
        for stale in admission.stale {
            self.release_owner(socket_owner(stale));
        }
        self.probe.ipc_drain(sender);
        dispatch_admitted(
            buffer,
            sender,
            socket_owner(Owner::of(&caller)),
            &mut self.iface,
            &mut self.device,
            &mut self.sockets,
            &mut self.table,
            &mut self.tls_table,
            &mut self.tls_pending,
            &mut self.pending_ready,
            &mut self.pending_dns,
            &self.resolver,
            &self.local_ip,
        );
        drain_ready_waits(&mut self.pending_ready, &mut self.sockets, &self.table);
        self.ipc_burst_grace = IPC_BURST_GRACE_YIELDS;
    }

    /// No valid trailer: a watched owner's root death, or nothing to serve.
    fn on_unattributed(&mut self, sender: usize, _payload: &[u8]) {
        self.probe.received();
        if let Some(owner) = self.watches.take_death(sender) {
            self.release_owner(socket_owner(owner));
        }
        self.probe.ipc_miss();
    }

    fn on_empty_poll(&mut self) {
        self.probe.ipc_miss();
        if consume_ipc_burst_grace(&mut self.ipc_burst_grace) {
            // Let the caller run after a reply before parking on NET_RX;
            // sequential IPC bursts then avoid one timer quantum per call.
            sys_yield();
        } else {
            let now = now_instant();
            let wait_ticks = match self.iface.poll_at(now, &self.sockets) {
                Some(at) if at > now => (at - now)
                    .total_millis()
                    .div_ceil(SCHEDULER_TICK_MS)
                    .clamp(1, NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS),
                Some(_) => 1,
                None => NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS,
            };
            self.probe.wait_net_rx(wait_ticks);
        }
    }
}

#[cfg(target_os = "none")]
impl NetState<'_> {
    /// Reclaim everything a dead owner generation holds. Its watch is already
    /// gone: consumed by the kernel with the death, or cancelled when a newer
    /// generation was watched.
    fn release_owner(&mut self, owner: SocketOwner) {
        if self.device.release_port(owner) {
            println("[net-bridge] guest port released");
        }
        let (aborted, released) = reclaim_owner_state(
            owner,
            &mut self.sockets,
            &mut self.table,
            &mut self.tls_table,
            &mut self.tls_pending,
            &mut self.pending_ready,
            &mut self.pending_dns,
            &self.resolver,
            sys_get_time().saturating_add(ABORT_DRAIN_TICKS),
        );
        if let Some(line) = owner_death_line(owner, aborted, released) {
            println(&line);
        }
    }
}

#[cfg(test)]
#[path = "service-runtime-tests.rs"]
mod tests;

#[cfg(test)]
#[path = "service-runtime-admission-tests.rs"]
mod admission_tests;

#[cfg(test)]
#[path = "service-runtime-owner-death-tests.rs"]
mod owner_death_tests;
