use super::*;
use smoltcp::iface::{Config, Interface, SocketSet, SocketStorage};
use smoltcp::socket::udp as smoltcp_udp;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress};

fn setup_test_env() -> (
    VirtioNetDevice,
    [SocketStorage<'static>; 4],
    Interface,
    SocketTable,
    BTreeMap<u64, TlsSocketEntry>,
) {
    let mut device = VirtioNetDevice::new();
    let cfg = Config::new(HardwareAddress::Ethernet(EthernetAddress([
        0x52, 0x54, 0x00, 0x12, 0x34, 0x56,
    ])));
    let iface = Interface::new(cfg, &mut device, Instant::from_micros(0));
    let storage = [SocketStorage::EMPTY; 4];
    let table = SocketTable::new();
    let tls_table = BTreeMap::new();
    (device, storage, iface, table, tls_table)
}

#[test]
fn test_handlers_cross_owner_tcp_and_udp_rejection() {
    let (mut device, mut storage, mut iface, mut table, mut tls_table) = setup_test_env();
    let mut sockets = SocketSet::new(&mut storage[..]);
    let owner_a = SocketOwner {
        cell_id: 10,
        generation: 1,
    };
    let owner_b = SocketOwner {
        cell_id: 20,
        generation: 1,
    };
    let local_ip = [10, 0, 2, 15];

    let (handle, cap_id) = make_tcp(&mut sockets, &mut table, owner_a).expect("make_tcp");
    table.set_state(cap_id, SocketState::Listening);
    // Foreign owner B calls TcpClose through handler dispatch -> socket is NOT removed
    let close_req = NetRequest::TcpClose {
        cap_id: cap_id as u32,
    };
    assert!(tcp::handle_tcp_request(
        &close_req,
        1,
        owner_b,
        &mut iface,
        &mut sockets,
        &mut table,
        &mut tls_table,
    ));
    assert_eq!(table.get(cap_id, owner_a), Some(handle));
    assert_eq!(table.get(cap_id, owner_b), None);

    // UDP cross-owner handler dispatch
    let udp_h = sockets.add(smoltcp_udp::Socket::new(
        smoltcp_udp::PacketBuffer::new(
            alloc::vec![smoltcp_udp::PacketMetadata::EMPTY; 2],
            alloc::vec![0u8; 128],
        ),
        smoltcp_udp::PacketBuffer::new(
            alloc::vec![smoltcp_udp::PacketMetadata::EMPTY; 2],
            alloc::vec![0u8; 128],
        ),
    ));
    let udp_cap = table.insert(udp_h, owner_a).expect("insert udp");
    table.mark_udp(udp_cap);

    // Foreign owner B calls UdpBind through handler dispatch -> rejected, state unaffected
    let bind_req = NetRequest::UdpBind {
        cap_id: udp_cap as u32,
        port: 9000,
    };
    assert!(super::udp::handle_udp_request(
        &bind_req,
        1,
        owner_b,
        &mut iface,
        &mut device,
        &mut sockets,
        &mut table,
    ));
    assert_ne!(
        table.get_state(udp_cap, owner_a),
        Some(SocketState::Listening)
    );
    assert_eq!(table.get(udp_cap, owner_b), None);
}
