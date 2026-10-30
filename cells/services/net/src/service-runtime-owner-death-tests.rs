//! Owner death: what the service does with a dead client generation's sockets,
//! waits and queries (`reclaim_owner_state`, `reap_aborted`, `owner_for_root`).
//! The wire behaviour is read from real smoltcp sockets talking over
//! `smoltcp::phy::Loopback`; the kernel half (watch grant, `TryRecv` delivery)
//! is tested in the kernel crate and the `run()` loop itself is bare-metal only.

use super::*;
use smoltcp::{
    iface::SocketHandle,
    phy::{Loopback, Medium},
    socket::tcp,
    wire::IpEndpoint,
};

const OWNER: SocketOwner = SocketOwner { cell_id: 10, generation: 1 };
const NEXT_GENERATION: SocketOwner = SocketOwner { cell_id: 10, generation: 2 };
const OTHER_CELL: SocketOwner = SocketOwner { cell_id: 20, generation: 1 };
const SERVER_PORT: u16 = 80;
const PEER: [u8; 4] = [10, 0, 0, 1];

/// One smoltcp interface whose frames loop back to itself, with the service
/// state a reclaim touches.
struct Stack {
    device: Loopback,
    iface: Interface,
    sockets: SocketSet<'static>,
    table: SocketTable,
    tls_table: BTreeMap<u64, TlsSocketEntry>,
    tls_pending: crate::tls_handler::TlsPending,
    pending_ready: BTreeMap<SocketOwner, PendingReady>,
    pending_dns: heapless::Vec<PendingDns, DNS_QUERY_LIMIT>,
    resolver: Resolver,
    clock_ms: i64,
    next_port: u16,
}

impl Stack {
    fn new() -> Self {
        let mut device = Loopback::new(Medium::Ethernet);
        let mut iface = Interface::new(
            Config::new(HardwareAddress::Ethernet(MAC)),
            &mut device,
            Instant::from_millis(0),
        );
        iface.update_ip_addrs(|addresses| {
            let _ = addresses.push(IpCidr::new(IpAddress::v4(10, 0, 0, 1), 24));
        });
        let mut sockets = SocketSet::new(alloc::vec::Vec::new());
        let resolver = Resolver::install(&mut sockets);
        Self {
            device,
            iface,
            sockets,
            table: SocketTable::new(),
            tls_table: BTreeMap::new(),
            tls_pending: Default::default(),
            pending_ready: BTreeMap::new(),
            pending_dns: heapless::Vec::new(),
            resolver,
            clock_ms: 0,
            next_port: 49_152,
        }
    }

    /// `rounds` network turns, 10 ms apart.
    fn pump(&mut self, rounds: usize) {
        for _ in 0..rounds {
            self.advance(10);
        }
    }

    fn advance(&mut self, ms: i64) {
        self.clock_ms += ms;
        self.iface.poll(Instant::from_millis(self.clock_ms), &mut self.device, &mut self.sockets);
    }

    fn tcp(&mut self) -> SocketHandle {
        self.sockets.add(tcp::Socket::new(
            tcp::SocketBuffer::new(alloc::vec![0u8; 256]),
            tcp::SocketBuffer::new(alloc::vec![0u8; 256]),
        ))
    }

    fn socket(&mut self, handle: SocketHandle) -> &mut tcp::Socket<'static> {
        self.sockets.get_mut::<tcp::Socket>(handle)
    }

    /// A connecting client socket owned by `owner`; no network turn has run.
    fn connecting(&mut self, owner: SocketOwner) -> (u64, SocketHandle) {
        let handle = self.tcp();
        let port = self.next_port;
        self.next_port += 1;
        let remote = IpEndpoint::new(IpAddress::v4(PEER[0], PEER[1], PEER[2], PEER[3]), SERVER_PORT);
        self.sockets
            .get_mut::<tcp::Socket>(handle)
            .connect(self.iface.context(), remote, port)
            .expect("connect");
        (self.table.insert(handle, owner).expect("cap"), handle)
    }

    /// An established connection: `owner`'s client cap and the peer's socket.
    fn established(&mut self, owner: SocketOwner) -> (u64, SocketHandle, SocketHandle) {
        let server = self.tcp();
        self.socket(server).listen(SERVER_PORT).expect("listen");
        let (cap, client) = self.connecting(owner);
        for _ in 0..50 {
            if self.socket(client).state() == tcp::State::Established
                && self.socket(server).state() == tcp::State::Established
            {
                return (cap, client, server);
            }
            self.pump(1);
        }
        panic!(
            "no handshake over loopback: client {:?}, server {:?}",
            self.socket(client).state(),
            self.socket(server).state()
        );
    }

    fn reclaim(&mut self, owner: SocketOwner, deadline: u64) -> (usize, usize) {
        reclaim_owner_state(
            owner,
            &mut self.sockets,
            &mut self.table,
            &mut self.tls_table,
            &mut self.tls_pending,
            &mut self.pending_ready,
            &mut self.pending_dns,
            &self.resolver,
            deadline,
        )
    }

    fn sockets_alive(&self) -> usize {
        self.sockets.iter().count()
    }
}

fn pending_ready(op_id: usize) -> PendingReady {
    PendingReady { op_id, interests: heapless::Vec::new(), cursor: 0, deadline: u64::MAX }
}

#[test]
fn silent_removal_leaves_the_peer_established_and_abort_resets_it() {
    // Control: dropping the dead owner's socket with `SocketSet::remove` (what
    // the unreachable reclaim did) tells the peer nothing.
    let mut control = Stack::new();
    let (cap, client, server) = control.established(OWNER);
    let handle = control.table.remove_internal(cap).expect("cap");
    assert_eq!(handle, client);
    control.sockets.remove(handle);
    control.pump(20);
    assert_eq!(control.socket(server).state(), tcp::State::Established);
    assert!(control.socket(server).may_recv(), "no FIN, no RST: still open");

    // Owner death: one RST reaches the peer, which is closed by it.
    let mut stack = Stack::new();
    let (cap, client, server) = stack.established(OWNER);
    assert_eq!(stack.reclaim(OWNER, u64::MAX), (1, 0));
    assert!(stack.table.is_aborting(cap));
    assert!(stack.socket(client).remote_endpoint().is_some(), "RST still owed");
    stack.pump(2);
    assert_eq!(stack.socket(server).state(), tcp::State::Closed, "peer was reset");
    assert!(!stack.socket(server).may_recv());
    assert!(stack.socket(server).recv(|_| (0, ())).is_err());
    // smoltcp forgets the connection once the RST has gone: the entry is satisfied.
    assert!(stack.socket(client).remote_endpoint().is_none());
    let alive = stack.sockets_alive();
    reap_aborted(&mut stack.table, &mut stack.sockets, 0);
    assert!(!stack.table.is_aborting(cap));
    assert!(stack.table.get_unchecked(cap).is_none());
    assert_eq!(stack.sockets_alive(), alive - 1);
}

#[test]
fn reap_aborted_keeps_an_entry_until_its_rst_leaves_and_drops_it_at_the_deadline() {
    let mut stack = Stack::new();
    let (cap, handle) = stack.connecting(OWNER);
    assert_eq!(stack.reclaim(OWNER, 1_000), (1, 0));
    let alive = stack.sockets_alive();

    // No frame has left (no network turn ran): before the deadline it stays.
    reap_aborted(&mut stack.table, &mut stack.sockets, 999);
    assert!(stack.table.is_aborting(cap));
    assert_eq!(stack.sockets_alive(), alive);
    assert!(stack.socket(handle).remote_endpoint().is_some());

    // Deadline: removed silently and the table is back to baseline.
    reap_aborted(&mut stack.table, &mut stack.sockets, 1_000);
    assert!(!stack.table.is_aborting(cap));
    assert!(stack.table.get_unchecked(cap).is_none());
    assert_eq!(stack.sockets_alive(), alive - 1);
    assert!(stack.table.owned_cap(OWNER).is_none());
    assert_eq!(stack.table.aborting().count(), 0);

    // With the RST sent, the entry is finished well before its deadline.
    let (cap, _) = stack.connecting(OWNER);
    assert_eq!(stack.reclaim(OWNER, u64::MAX), (1, 0));
    stack.pump(2);
    reap_aborted(&mut stack.table, &mut stack.sockets, 0);
    assert!(stack.table.get_unchecked(cap).is_none());
    assert_eq!(stack.table.aborting().count(), 0);
}

#[test]
fn reclaim_frees_everything_the_dead_owner_held_and_nothing_else() {
    let mut stack = Stack::new();
    // The dead generation: a connection, a UDP socket, a listener, a pending
    // ready wait and pending DNS queries.
    let (connect_cap, _connect) = stack.connecting(OWNER);
    let udp = stack.sockets.add(smoltcp::socket::udp::Socket::new(
        smoltcp::socket::udp::PacketBuffer::new(
            alloc::vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 1],
            alloc::vec![0u8; 64],
        ),
        smoltcp::socket::udp::PacketBuffer::new(
            alloc::vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 1],
            alloc::vec![0u8; 64],
        ),
    ));
    let udp_cap = stack.table.insert(udp, OWNER).expect("udp cap");
    stack.table.mark_udp(udp_cap);
    let listener = stack.tcp();
    stack.socket(listener).listen(8080).expect("listen");
    let listener_cap = stack.table.insert(listener, OWNER).expect("listener cap");
    stack.pending_ready.insert(OWNER, pending_ready(1));
    // The same CellId's next generation and another cell: untouched.
    let (successor_cap, successor) = stack.connecting(NEXT_GENERATION);
    let (foreign_cap, foreign) = stack.connecting(OTHER_CELL);
    stack.pending_ready.insert(NEXT_GENERATION, pending_ready(2));
    stack.pending_ready.insert(OTHER_CELL, pending_ready(3));

    // DNS: the dead owner has 3 queries in flight, another cell has 2.
    let mut dead_queries = alloc::vec::Vec::new();
    let mut live_queries = alloc::vec::Vec::new();
    for (owner, count) in [(OWNER, 3), (OTHER_CELL, 2)] {
        for _ in 0..count {
            let query = stack
                .resolver
                .start("host.example", &mut stack.iface, &mut stack.sockets)
                .expect("dns query");
            stack
                .pending_dns
                .push(PendingDns {
                    owner,
                    sender: 1,
                    op_id: None,
                    query: Some(query),
                    answer: None,
                    deadline: u64::MAX,
                })
                .ok()
                .expect("pending dns slot");
            if owner == OWNER { dead_queries.push(query) } else { live_queries.push(query) }
        }
    }
    let alive = stack.sockets_alive();

    // One RST owed (the connection); UDP and listener freed on the spot.
    assert_eq!(stack.reclaim(OWNER, 500), (1, 2));
    assert_eq!(stack.sockets_alive(), alive - 2);
    for gone in [udp_cap, listener_cap] {
        assert!(stack.table.get_unchecked(gone).is_none(), "cap {gone}");
    }
    assert!(stack.table.is_aborting(connect_cap));
    assert_eq!(stack.table.aborting().count(), 1);
    // "Nothing of the owner remains except ledger entries."
    assert_eq!(stack.table.owned_cap(OWNER), Some(connect_cap));
    assert!(!stack.pending_ready.contains_key(&OWNER));
    assert_eq!(stack.pending_ready.len(), 2);
    assert!(stack.pending_dns.iter().all(|pending| pending.owner != OWNER));
    assert_eq!(stack.pending_dns.len(), 2);
    // The dead owner's queries were cancelled in the resolver (a free slot panics
    // on any access); the other cell's are still pending.
    for query in dead_queries {
        let freed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            stack.resolver.poll_result(query, &mut stack.sockets)
        }));
        assert!(freed.is_err(), "dead owner's DNS query slot was freed");
    }
    for query in live_queries {
        assert_eq!(stack.resolver.poll_result(query, &mut stack.sockets), None, "still pending");
    }

    // The successor generation and the other cell keep their sockets and caps.
    assert_eq!(stack.table.get(successor_cap, NEXT_GENERATION), Some(successor));
    assert_eq!(stack.table.get(foreign_cap, OTHER_CELL), Some(foreign));
    assert_eq!(stack.socket(successor).state(), tcp::State::SynSent);
    assert_eq!(stack.socket(foreign).state(), tcp::State::SynSent);
    // Repeating the reclaim is a no-op.
    assert_eq!(stack.reclaim(OWNER, 500), (0, 0));
    assert_eq!(stack.table.aborting().count(), 1);
}

#[test]
fn a_graceful_close_in_flight_is_flushed_not_pre_empted_and_only_its_reaper_ends_it() {
    let mut stack = Stack::new();
    let (cap, client, server) = stack.established(OWNER);
    // The owner asked for FIN after its output, then died.
    stack.socket(client).close();
    stack.table.begin_graceful_close(cap, u64::MAX);
    assert_eq!(stack.reclaim(OWNER, u64::MAX), (0, 0));
    assert!(!stack.table.is_aborting(cap));
    assert!(stack.table.is_graceful_closing(cap));
    assert_eq!(stack.table.aborting().count(), 0);

    // The FIN goes out: the peer sees an orderly close, not a reset.
    stack.pump(4);
    assert_eq!(stack.socket(server).state(), tcp::State::CloseWait);
    assert!(!stack.socket(server).may_recv(), "orderly close: the peer saw a FIN");
    // The abort reaper never touches it; the graceful reaper keeps it while the
    // close is still running (TIME-WAIT) and ends it once the socket is Closed.
    stack.socket(server).close();
    stack.pump(4);
    assert_eq!(stack.socket(client).state(), tcp::State::TimeWait);
    reap_aborted(&mut stack.table, &mut stack.sockets, u64::MAX);
    assert_eq!(reap_graceful_closes(&mut stack.table, &mut stack.sockets, 0), 0);
    assert!(stack.table.is_graceful_closing(cap), "still closing");
    stack.advance(11_000);
    stack.advance(10);
    assert_eq!(stack.socket(client).state(), tcp::State::Closed);
    assert_eq!(reap_graceful_closes(&mut stack.table, &mut stack.sockets, 0), 0);
    assert!(stack.table.get_unchecked(cap).is_none());
    assert!(stack.table.owned_cap(OWNER).is_none());
}

#[test]
fn owner_for_root_matches_the_root_tid_exactly() {
    let mut watched = BTreeMap::new();
    watched.insert(OWNER, (100usize, 1u64));
    watched.insert(OTHER_CELL, (200, 2));
    assert_eq!(owner_for_root(&watched, 100), Some(OWNER));
    assert_eq!(owner_for_root(&watched, 200), Some(OTHER_CELL));
    // A worker thread of the Cell exiting is not root death.
    assert_eq!(owner_for_root(&watched, 101), None);
    assert_eq!(owner_for_root(&watched, 0), None);
    assert_eq!(owner_for_root(&BTreeMap::new(), 100), None);
}

#[test]
fn a_new_generation_of_a_cell_releases_only_the_old_generation() {
    let mut watched = BTreeMap::new();
    watched.insert(OWNER, (100usize, 1u64));
    watched.insert(OTHER_CELL, (200, 2));
    assert_eq!(stale_generation_of(&watched, NEXT_GENERATION), Some(OWNER));
    // Never the same generation, never another cell.
    assert_eq!(stale_generation_of(&watched, OWNER), None);
    assert_eq!(stale_generation_of(&watched, SocketOwner { cell_id: 30, generation: 1 }), None);

    // And through the real state reclaim: the old generation goes, the new stays.
    let mut stack = Stack::new();
    let (old_cap, _) = stack.connecting(OWNER);
    let (new_cap, new_handle) = stack.connecting(NEXT_GENERATION);
    let stale = stale_generation_of(&watched, NEXT_GENERATION).expect("stale generation");
    assert_eq!(stack.reclaim(stale, u64::MAX), (1, 0));
    assert!(stack.table.is_aborting(old_cap));
    assert_eq!(stack.table.get(new_cap, NEXT_GENERATION), Some(new_handle));
    assert!(!stack.table.is_aborting(new_cap));
}

#[test]
fn the_log_line_appears_only_when_something_was_reclaimed() {
    assert_eq!(owner_death_line(OWNER, 0, 0), None);
    assert_eq!(
        owner_death_line(OWNER, 2, 3).as_deref(),
        Some("[net] owner death: cell 10 gen 1: aborted 2 TCP, released 3 other")
    );
    assert!(owner_death_line(OWNER, 0, 1).is_some());
    assert!(owner_death_line(OWNER, 1, 0).is_some());
}

// ---- graceful close deadline (NET-GCD) ----

/// The Tier-3 control shape: the client closed gracefully, its FIN is acknowledged, but
/// the peer's FIN is stuck behind queued data and a closed window nobody drains.
/// Returns the client cap and both handles, the client in `FinWait2`.
fn fin_wait2_behind_a_closed_window(
    stack: &mut Stack,
    owner: SocketOwner,
    deadline: u64,
) -> (u64, SocketHandle, SocketHandle) {
    let (cap, client, server) = stack.established(owner);
    stack.socket(server).send_slice(&[1u8; 256]).expect("fill the client's window");
    stack.pump(4);
    stack.socket(server).send_slice(&[2u8; 256]).expect("queue behind the closed window");
    stack.pump(4);
    assert_eq!(stack.socket(client).recv_queue(), 256, "client receive buffer is full");
    stack.socket(client).close();
    stack.table.begin_graceful_close(cap, deadline);
    stack.pump(10);
    assert_eq!(stack.socket(client).state(), tcp::State::FinWait2);
    assert_eq!(stack.socket(server).state(), tcp::State::CloseWait);
    assert!(stack.socket(server).send_queue() > 0, "the peer's data is stuck");
    (cap, client, server)
}

impl Stack {
    fn reap_graceful(&mut self, now: u64) -> usize {
        reap_graceful_closes(&mut self.table, &mut self.sockets, now)
    }
}

#[test]
fn graceful_end_decides_every_state() {
    use GracefulEnd::{Abort as A, Free as F, Keep as K};
    use tcp::State;
    // Columns: (rst_owed, expired) = (no, no), (no, yes), (yes, no), (yes, yes).
    let expected = [
        (State::Closed, [F, F, A, A]),
        (State::Listen, [K, A, K, A]),
        (State::SynSent, [K, A, K, A]),
        (State::SynReceived, [K, A, K, A]),
        (State::Established, [K, A, K, A]),
        (State::FinWait1, [K, A, K, A]),
        (State::FinWait2, [K, A, K, A]),
        (State::CloseWait, [K, A, K, A]),
        (State::Closing, [K, A, K, A]),
        (State::LastAck, [K, A, K, A]),
        (State::TimeWait, [K, F, K, F]),
    ];
    for (state, row) in expected {
        for (column, (owed, expired)) in
            [(false, false), (false, true), (true, false), (true, true)].into_iter().enumerate()
        {
            assert_eq!(
                graceful_end(state, owed, expired),
                row[column],
                "{state:?} owed={owed} expired={expired}"
            );
        }
    }
}

#[test]
fn fin_wait2_peer_with_data_behind_a_closed_window_is_reset_at_the_deadline() {
    let mut stack = Stack::new();
    let (cap, client, server) = fin_wait2_behind_a_closed_window(&mut stack, OWNER, 10_000);
    let alive = stack.sockets_alive();

    // Inside the promise: kept, nothing sent.
    assert_eq!(stack.reap_graceful(9_999), 0);
    assert!(stack.table.is_graceful_closing(cap));
    assert_eq!(stack.socket(client).state(), tcp::State::FinWait2);
    stack.pump(4);
    assert_eq!(stack.socket(server).state(), tcp::State::CloseWait, "the peer heard nothing");

    // The deadline: aborted, the RST is owed and the slot is held until it left.
    assert_eq!(stack.reap_graceful(10_000), 1);
    assert_eq!(stack.socket(client).state(), tcp::State::Closed);
    assert!(stack.socket(client).remote_endpoint().is_some(), "RST still owed");
    assert!(stack.table.is_aborting(cap));
    assert!(!stack.table.is_graceful_closing(cap));
    assert_eq!(stack.table.get_unchecked(cap), Some(client));
    assert_eq!(stack.sockets_alive(), alive);
    // Not drained yet: the abort ledger keeps it.
    reap_aborted(&mut stack.table, &mut stack.sockets, 10_000);
    assert!(stack.table.is_aborting(cap));

    // One network turn pair emits the RST: the peer is closed by it, not left half-open.
    stack.pump(2);
    assert_eq!(stack.socket(server).state(), tcp::State::Closed, "peer was reset");
    assert!(stack.socket(server).recv(|_| (0, ())).is_err());
    assert!(stack.socket(client).remote_endpoint().is_none());
    reap_aborted(&mut stack.table, &mut stack.sockets, 10_000);
    assert!(stack.table.get_unchecked(cap).is_none());
    assert_eq!(stack.table.aborting().count(), 0);
    assert_eq!(stack.table.graceful_closes().count(), 0);
    assert_eq!(stack.sockets_alive(), alive - 1);
}

#[test]
fn characterisation_a_silent_removal_resets_the_peer_only_on_its_next_segment() {
    // (i) The old reaper's action on a FIN-WAIT-2 socket: the peer is told nothing.
    let mut quiet = Stack::new();
    let (cap, client, server) = quiet.established(OWNER);
    quiet.socket(client).close();
    quiet.pump(10);
    assert_eq!(quiet.socket(client).state(), tcp::State::FinWait2);
    let handle = quiet.table.remove_internal(cap).expect("cap");
    assert_eq!(handle, client);
    quiet.sockets.remove(handle);
    quiet.pump(20);
    assert_eq!(quiet.socket(server).state(), tcp::State::CloseWait, "nobody told the peer");

    // (ii) The same, but the peer speaks: the interface answers its segment with an RST.
    let mut talking = Stack::new();
    let (cap, client, server) = talking.established(OWNER);
    talking.socket(client).close();
    talking.pump(10);
    assert_eq!(talking.socket(client).state(), tcp::State::FinWait2);
    talking.socket(server).send_slice(b"late").expect("peer data");
    let handle = talking.table.remove_internal(cap).expect("cap");
    talking.sockets.remove(handle);
    talking.pump(4);
    assert_eq!(talking.socket(server).state(), tcp::State::Closed, "reset, but only because it spoke");
}

#[test]
fn a_time_wait_or_closed_socket_is_freed_at_the_deadline_without_an_rst() {
    let mut stack = Stack::new();
    let (cap, client, server) = stack.established(OWNER);
    stack.socket(client).close();
    stack.table.begin_graceful_close(cap, 100);
    stack.pump(4);
    stack.socket(server).close();
    stack.pump(4);
    assert_eq!(stack.socket(client).state(), tcp::State::TimeWait);
    assert_eq!(stack.socket(server).state(), tcp::State::Closed);
    let alive = stack.sockets_alive();

    assert_eq!(stack.reap_graceful(99), 0);
    assert!(stack.table.is_graceful_closing(cap), "TIME-WAIT inside the deadline is kept");
    assert_eq!(stack.reap_graceful(100), 0, "nothing is owed: no abort");
    assert!(stack.table.get_unchecked(cap).is_none());
    assert_eq!(stack.table.aborting().count(), 0);
    assert_eq!(stack.table.graceful_closes().count(), 0);
    assert_eq!(stack.sockets_alive(), alive - 1);
}

#[test]
fn fin_wait1_and_last_ack_are_aborted_at_the_deadline() {
    // FIN-WAIT-1: the FIN has not been acknowledged in time.
    let mut stack = Stack::new();
    let (cap, client, server) = stack.established(OWNER);
    stack.socket(client).close();
    assert_eq!(stack.socket(client).state(), tcp::State::FinWait1);
    stack.table.begin_graceful_close(cap, 100);
    assert_eq!(stack.reap_graceful(100), 1);
    assert!(stack.table.is_aborting(cap));
    stack.pump(2);
    assert_eq!(stack.socket(server).state(), tcp::State::Closed, "FIN-WAIT-1 peer reset");

    // LAST-ACK: the passive side closed and its final ACK never came.
    let mut stack = Stack::new();
    let (cap, client, server) = stack.established(OWNER);
    stack.socket(server).close();
    stack.pump(4);
    assert_eq!(stack.socket(client).state(), tcp::State::CloseWait);
    stack.socket(client).close();
    assert_eq!(stack.socket(client).state(), tcp::State::LastAck);
    stack.table.begin_graceful_close(cap, 100);
    assert_eq!(stack.reap_graceful(100), 1);
    assert!(stack.table.is_aborting(cap));
    stack.pump(2);
    assert_eq!(stack.socket(server).state(), tcp::State::Closed, "LAST-ACK peer reset");
}

#[test]
fn a_close_that_finishes_before_its_deadline_keeps_its_output_and_is_never_reset() {
    let mut stack = Stack::new();
    let (cap, client, server) = stack.established(OWNER);
    stack.socket(client).send_slice(&[7u8; 100]).expect("output");
    stack.socket(client).close();
    stack.table.begin_graceful_close(cap, 10_000);
    stack.pump(6);
    let mut got = [0u8; 128];
    let read = stack.socket(server).recv_slice(&mut got).expect("flushed output");
    assert_eq!(read, 100);
    assert_eq!(got[..100], [7u8; 100]);
    assert!(!stack.socket(server).may_recv(), "orderly end: a FIN followed the data");
    stack.socket(server).close();
    stack.pump(4);
    assert_eq!(stack.socket(client).state(), tcp::State::TimeWait);
    assert_eq!(stack.reap_graceful(5_000), 0);
    assert!(stack.table.is_graceful_closing(cap), "TIME-WAIT is kept inside the deadline");

    stack.advance(11_000);
    stack.advance(10);
    assert_eq!(stack.socket(client).state(), tcp::State::Closed);
    assert!(stack.socket(client).remote_endpoint().is_none());
    assert_eq!(stack.reap_graceful(5_000), 0, "freed inside the deadline, nothing aborted");
    assert!(stack.table.get_unchecked(cap).is_none());
    assert_eq!(stack.socket(server).state(), tcp::State::Closed);
    assert_eq!(stack.table.aborting().count(), 0);
}

#[test]
fn an_rst_that_smoltcp_already_owes_is_not_dropped_by_the_graceful_reaper() {
    let mut stack = Stack::new();
    let (cap, client, server) = stack.established(OWNER);
    stack.socket(client).close();
    stack.socket(client).abort();
    assert!(stack.socket(client).remote_endpoint().is_some());
    stack.table.begin_graceful_close(cap, u64::MAX);

    assert_eq!(stack.reap_graceful(0), 1, "an owed RST ends the graceful entry at once");
    assert!(stack.table.is_aborting(cap));
    assert!(!stack.table.is_graceful_closing(cap));
    assert_eq!(stack.table.get_unchecked(cap), Some(client));
    stack.pump(2);
    assert_eq!(stack.socket(server).state(), tcp::State::Closed);
    reap_aborted(&mut stack.table, &mut stack.sockets, 0);
    assert!(stack.table.get_unchecked(cap).is_none());
}

#[test]
fn graceful_reap_is_bounded_per_turn_and_finishes_every_entry_once() {
    let mut stack = Stack::new();
    let mut pairs = alloc::vec::Vec::new();
    for _ in 0..20 {
        pairs.push(stack.established(OWNER));
    }
    for &(cap, client, _) in &pairs {
        stack.socket(client).close();
        stack.table.begin_graceful_close(cap, 100);
    }
    let alive = stack.sockets_alive();
    assert_eq!(GRACEFUL_REAP_PER_TURN, 8);

    for (aborted, graceful, aborting) in [(8, 12, 8), (8, 4, 16), (4, 0, 20), (0, 0, 20)] {
        assert_eq!(stack.reap_graceful(100), aborted);
        assert_eq!(stack.table.graceful_closes().count(), graceful);
        assert_eq!(stack.table.aborting().count(), aborting);
    }

    stack.pump(4);
    for &(_, _, server) in &pairs {
        assert_eq!(stack.socket(server).state(), tcp::State::Closed);
    }
    for _ in 0..2 {
        reap_aborted(&mut stack.table, &mut stack.sockets, 100);
    }
    assert_eq!(stack.table.aborting().count(), 0);
    assert!(stack.table.owned_cap(OWNER).is_none());
    assert_eq!(stack.sockets_alive(), alive - 20);
}

#[test]
fn a_dead_owner_graceful_cap_is_flushed_then_aborted_at_its_deadline() {
    let mut stack = Stack::new();
    let (cap, client, server) = fin_wait2_behind_a_closed_window(&mut stack, OWNER, 10_000);
    let alive = stack.sockets_alive();

    // The owner died: its graceful cap is skipped, and the death logs nothing.
    assert_eq!(stack.reclaim(OWNER, u64::MAX), (0, 0));
    assert_eq!(owner_death_line(OWNER, 0, 0), None);
    assert!(stack.table.is_graceful_closing(cap));
    assert_eq!(stack.socket(client).state(), tcp::State::FinWait2);

    // Its own deadline ends it with the RST, and the line says why.
    let aborted = stack.reap_graceful(10_000);
    assert_eq!(aborted, 1);
    assert_eq!(
        graceful_deadline_line(aborted).as_deref(),
        Some("[net] graceful close deadline: aborted 1 TCP")
    );
    stack.pump(2);
    assert_eq!(stack.socket(server).state(), tcp::State::Closed, "peer was reset");
    reap_aborted(&mut stack.table, &mut stack.sockets, 10_000);
    assert!(stack.table.get_unchecked(cap).is_none());
    assert_eq!(stack.table.aborting().count(), 0);
    assert_eq!(stack.table.graceful_closes().count(), 0);
    assert!(stack.table.owned_cap(OWNER).is_none());
    assert_eq!(stack.sockets_alive(), alive - 1);
}

#[test]
fn the_deadline_line_appears_only_when_something_was_aborted() {
    assert_eq!(graceful_deadline_line(0), None);
    assert_eq!(
        graceful_deadline_line(1).as_deref(),
        Some("[net] graceful close deadline: aborted 1 TCP")
    );
    assert_eq!(
        graceful_deadline_line(3).as_deref(),
        Some("[net] graceful close deadline: aborted 3 TCP")
    );
}
