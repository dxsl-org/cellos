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
    reap_graceful_closes(&mut stack.table, &mut stack.sockets);
    assert!(stack.table.is_graceful_closing(cap), "still closing");
    stack.advance(11_000);
    stack.advance(10);
    assert_eq!(stack.socket(client).state(), tcp::State::Closed);
    reap_graceful_closes(&mut stack.table, &mut stack.sockets);
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
