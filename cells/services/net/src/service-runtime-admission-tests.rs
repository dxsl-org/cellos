//! Runtime admission: the real gate (`admit_request`) followed, only when it
//! admits, by the real intercepts and dispatch (`dispatch_admitted`) — the order
//! the service loop runs them in. Effects are read back from the service state
//! itself: socket table, socket set, DNS pending set, ready waits and the NIC
//! TX queue.

use super::*;
use api::caller_identity::{
    CALLER_FLAG_NET_CONNECT, CALLER_FLAG_NET_L2, CALLER_FLAG_NET_LISTEN,
    CALLER_FLAG_NET_RESOLVE, CALLER_FLAG_NET_RIGHTS_MASK,
};
use smoltcp::socket::udp;

use crate::socket_state::SocketState;
use crate::tls_wire::TLS_CONNECT_OP;

/// A TID no host file descriptor can match: host replies fail with `EBADF`
/// instead of touching a real descriptor.
const SENDER: usize = 0x3FFF_FFF0;
const OWNER: SocketOwner = SocketOwner { cell_id: 10, generation: 1 };
const FOREIGN: SocketOwner = SocketOwner { cell_id: 20, generation: 1 };
const LOCAL_IP: [u8; 4] = [10, 0, 2, 15];

struct Net {
    device: VirtioNetDevice,
    iface: Interface,
    sockets: SocketSet<'static>,
    table: SocketTable,
    tls_table: BTreeMap<u64, TlsSocketEntry>,
    tls_pending: crate::tls_handler::TlsPending,
    pending_ready: BTreeMap<SocketOwner, PendingReady>,
    pending_dns: heapless::Vec<PendingDns, DNS_QUERY_LIMIT>,
    resolver: Resolver,
}

/// Observable service state a request could change.
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    next_cap: u64,
    sockets: usize,
    pending_dns: usize,
    pending_ready: usize,
    tx_frames: usize,
    guest_mac: Option<[u8; 6]>,
}

impl Net {
    fn new() -> Self {
        let mut device = VirtioNetDevice::new();
        let config = Config::new(HardwareAddress::Ethernet(MAC));
        let mut iface = Interface::new(config, &mut device, Instant::from_micros(0));
        iface.update_ip_addrs(|addresses| {
            let _ = addresses.push(IpCidr::new(IpAddress::v4(10, 0, 2, 15), 24));
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
        }
    }

    /// One attested request, served in loop order. Returns whether it was admitted.
    fn serve(&mut self, owner: SocketOwner, flags: u32, buffer: &[u8]) -> bool {
        if !admit_request(buffer, SENDER, flags) {
            return false;
        }
        dispatch_admitted(
            buffer, SENDER, owner, &mut self.iface, &mut self.device, &mut self.sockets,
            &mut self.table, &mut self.tls_table, &mut self.tls_pending,
            &mut self.pending_ready, &mut self.pending_dns, &self.resolver, &LOCAL_IP,
        );
        true
    }

    fn typed(&mut self, owner: SocketOwner, flags: u32, request: &NetRequest<'_>) -> bool {
        let mut buffer = [0u8; IPC_BUF_SIZE];
        ipc::encode(request, &mut buffer).expect("encode");
        self.serve(owner, flags, &buffer)
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            next_cap: self.table.next_cap_for_test(),
            sockets: self.sockets.iter().count(),
            pending_dns: self.pending_dns.len(),
            pending_ready: self.pending_ready.len(),
            tx_frames: self.device.tx_queue_len(),
            guest_mac: self.device.guest_mac(),
        }
    }

    /// Flush whatever the stack already owes the wire, so a later TX count
    /// reflects only the request under test.
    fn settle(&mut self) {
        self.iface.poll(now_instant(), &mut self.device, &mut self.sockets);
    }

    fn own_udp(&mut self, owner: SocketOwner) -> (u64, smoltcp::iface::SocketHandle) {
        let cap = self.table.next_cap_for_test();
        assert!(self.typed(owner, CALLER_FLAG_NET_CONNECT, &NetRequest::UdpCreate));
        let handle = self.table.get(cap, owner).expect("UdpCreate allocated a socket");
        (cap, handle)
    }
}

/// Gated request without its right: denied, and nothing in the service moved.
fn assert_denied_without_effect(net: &mut Net, request: &NetRequest<'_>, right: u32) {
    net.settle();
    let before = net.snapshot();
    for flags in [0, CALLER_FLAG_NET_RIGHTS_MASK & !right, !right] {
        assert!(!net.typed(OWNER, flags, request), "{request:?} admitted with {flags:#x}");
        assert_eq!(net.snapshot(), before, "{request:?} denied but changed state");
    }
    // Nothing deferred either: the next stack turn has no frame to send for it.
    net.settle();
    assert_eq!(net.snapshot(), before, "{request:?} denied but left work for the wire");
}

#[test]
fn socket_creating_requests_are_denied_before_allocation_and_admitted_with_the_right() {
    let mut net = Net::new();
    let cases: [(NetRequest<'_>, u32); 3] = [
        (NetRequest::TcpConnect { addr: [10, 0, 2, 2], port: 80 }, CALLER_FLAG_NET_CONNECT),
        (NetRequest::UdpCreate, CALLER_FLAG_NET_CONNECT),
        (NetRequest::TcpListen { port: 8080 }, CALLER_FLAG_NET_LISTEN),
    ];
    for (request, right) in &cases {
        assert_denied_without_effect(&mut net, request, *right);

        let cap = net.table.next_cap_for_test();
        assert!(net.typed(OWNER, *right, request));
        assert_eq!(net.table.next_cap_for_test(), cap + 1, "{request:?} allocated no cap");
    }
    // Sockets that need no reply round trip stay in the table.
    assert!(net.table.is_udp(2, OWNER));
    assert_eq!(net.table.get_state(3, OWNER), Some(SocketState::Listening));
}

#[test]
fn resolve_is_denied_before_any_query_and_admitted_with_the_right() {
    let mut net = Net::new();
    for hostname in ["example.com", "10.0.2.2", "gateway"] {
        assert_denied_without_effect(&mut net, &NetRequest::Resolve { hostname }, CALLER_FLAG_NET_RESOLVE);
    }

    assert!(net.typed(OWNER, CALLER_FLAG_NET_RESOLVE, &NetRequest::Resolve { hostname: "example.com" }));
    assert_eq!(net.pending_dns.len(), 1, "an admitted wire lookup retains its query");
    assert_eq!(net.pending_dns[0].owner, OWNER);
}

#[test]
fn udp_bind_and_send_on_an_owned_socket_still_need_their_rights() {
    let mut net = Net::new();
    let (cap, handle) = net.own_udp(OWNER);

    let bind = NetRequest::UdpBind { cap_id: cap as u32, port: 5000 };
    assert_denied_without_effect(&mut net, &bind, CALLER_FLAG_NET_LISTEN);
    assert!(!net.sockets.get::<udp::Socket>(handle).is_open());
    assert_ne!(net.table.get_state(cap, OWNER), Some(SocketState::Listening));
    assert!(net.typed(OWNER, CALLER_FLAG_NET_LISTEN, &bind));
    assert!(net.sockets.get::<udp::Socket>(handle).is_open());
    assert_eq!(net.table.get_state(cap, OWNER), Some(SocketState::Listening));

    let send = NetRequest::UdpSend { cap_id: cap as u32, addr: [10, 0, 2, 2], port: 9, data: b"ping" };
    // Denied: no datagram queued, so no ARP or UDP frame reaches the TX queue.
    assert_denied_without_effect(&mut net, &send, CALLER_FLAG_NET_CONNECT);
    net.settle();
    let frames = net.device.tx_queue_len();
    assert!(net.typed(OWNER, CALLER_FLAG_NET_CONNECT, &send));
    assert!(net.device.tx_queue_len() > frames, "an admitted UdpSend reaches the wire");
}

#[test]
fn multicast_join_is_denied_before_any_igmp_frame() {
    let mut net = Net::new();
    let join = NetRequest::MulticastJoin { cap_id: 0, group: [239, 1, 2, 3] };
    assert_denied_without_effect(&mut net, &join, CALLER_FLAG_NET_LISTEN);

    net.settle();
    let frames = net.device.tx_queue_len();
    assert!(net.typed(OWNER, CALLER_FLAG_NET_LISTEN, &join));
    assert!(net.device.tx_queue_len() > frames, "an admitted join reports to the group");
}

#[test]
fn raw_l2_is_denied_before_any_frame_or_guest_registration() {
    let mut net = Net::new();
    let mut frame = [0u8; 60];
    frame[..6].copy_from_slice(&[0xFF; 6]);
    frame[6..12].copy_from_slice(&[0x52, 0x54, 0x00, 0x00, 0x00, 0x07]);
    let sends = crate::handlers::L2_SEND_REQUESTS.load(Ordering::Relaxed);
    let recvs = crate::handlers::L2_RECV_REQUESTS.load(Ordering::Relaxed);

    assert_denied_without_effect(&mut net, &NetRequest::L2Send { data: &frame }, CALLER_FLAG_NET_L2);
    assert_denied_without_effect(
        &mut net,
        &NetRequest::L2Recv { guest_mac: [0x52, 0x54, 0, 0, 0, 9] },
        CALLER_FLAG_NET_L2,
    );
    assert_eq!(crate::handlers::L2_SEND_REQUESTS.load(Ordering::Relaxed), sends);
    assert_eq!(crate::handlers::L2_RECV_REQUESTS.load(Ordering::Relaxed), recvs);

    // Admitted, the bridge handles the frame: it learns the sender's address.
    assert!(net.typed(OWNER, CALLER_FLAG_NET_L2, &NetRequest::L2Send { data: &frame }));
    assert_eq!(crate::handlers::L2_SEND_REQUESTS.load(Ordering::Relaxed), sends + 1);
    assert_eq!(net.device.guest_mac(), Some([0x52, 0x54, 0x00, 0x00, 0x00, 0x07]));
}

#[test]
fn raw_tls_connect_is_denied_before_allocating_a_socket() {
    let mut net = Net::new();
    let mut buffer = [0u8; IPC_BUF_SIZE];
    buffer[0] = TLS_CONNECT_OP;
    buffer[9..13].copy_from_slice(&[10, 0, 2, 2]);
    buffer[13..15].copy_from_slice(&443u16.to_le_bytes());
    buffer[15..17].copy_from_slice(&4u16.to_le_bytes());
    buffer[17..21].copy_from_slice(b"host");

    let before = net.snapshot();
    for flags in [0, CALLER_FLAG_NET_RIGHTS_MASK & !CALLER_FLAG_NET_CONNECT] {
        assert!(!net.serve(OWNER, flags, &buffer));
        assert_eq!(net.snapshot(), before);
        assert!(net.tls_table.is_empty());
    }
}

#[test]
fn ungated_requests_on_a_foreign_cap_keep_their_existing_error() {
    let mut net = Net::new();
    let (udp_cap, udp_handle) = net.own_udp(OWNER);
    let listen_cap = net.table.next_cap_for_test();
    assert!(net.typed(OWNER, CALLER_FLAG_NET_LISTEN, &NetRequest::TcpListen { port: 7000 }));
    let listen_handle = net.table.get(listen_cap, OWNER).expect("listener");

    let mut interests = [0u8; ipc::NET_READY_RECORD_BYTES];
    interests[..4].copy_from_slice(&(listen_cap as u32).to_le_bytes());
    interests[4] = ipc::net_ready::ACCEPT;
    let tcp = listen_cap as u32;
    let udp = udp_cap as u32;
    let ungated = [
        NetRequest::TcpSend { cap_id: tcp, data: b"x" },
        NetRequest::TcpRecv { cap_id: tcp, buf_len: 8 },
        NetRequest::TcpAccept { cap_id: tcp },
        NetRequest::UdpRecv { cap_id: udp, buf_len: 8 },
        NetRequest::SocketState { cap_id: tcp },
        NetRequest::TcpReady { interests: &interests, cursor: 0, wait: true },
        NetRequest::TcpReady { interests: &interests, cursor: 0, wait: false },
        NetRequest::TcpRecvReady { cap_id: tcp, buf_len: 8 },
        NetRequest::TcpSendReady { cap_id: tcp, data: b"x" },
        NetRequest::TcpCloseGraceful { cap_id: tcp },
        NetRequest::TcpClose { cap_id: tcp },
        NetRequest::TcpClose { cap_id: udp },
        NetRequest::GetLocalIp,
    ];
    net.settle();
    let before = net.snapshot();
    for request in &ungated {
        // No rights at all: ungated requests reach dispatch, which still
        // refuses the foreign cap exactly as before.
        assert!(net.typed(FOREIGN, 0, request), "{request:?} must stay ungated");
        assert_eq!(net.snapshot(), before, "{request:?} touched a foreign socket");
        assert_eq!(net.table.get(listen_cap, OWNER), Some(listen_handle));
        assert_eq!(net.table.get_state(listen_cap, OWNER), Some(SocketState::Listening));
        assert!(!net.table.is_graceful_closing(listen_cap));
        assert_eq!(net.table.get(udp_cap, OWNER), Some(udp_handle));
    }
}
