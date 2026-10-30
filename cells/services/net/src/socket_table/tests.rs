use super::*;
use smoltcp::iface::{Config, Interface, SocketSet, SocketStorage};
use smoltcp::phy::{Loopback, Medium};
use smoltcp::socket::{tcp, udp};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint};

fn make_test_handle(sockets: &mut SocketSet<'_>) -> SocketHandle {
    sockets.add(tcp::Socket::new(
        tcp::SocketBuffer::new(alloc::vec![0u8; 128]),
        tcp::SocketBuffer::new(alloc::vec![0u8; 128]),
    ))
}

#[test]
fn test_socket_table_cross_owner_isolation() {
    let mut storage = [SocketStorage::EMPTY; 4];
    let mut sockets = SocketSet::new(&mut storage[..]);
    let h1 = make_test_handle(&mut sockets);
    let h2 = make_test_handle(&mut sockets);

    let mut table = SocketTable::new();
    let owner1 = SocketOwner {
        cell_id: 10,
        generation: 1,
    };
    let owner2 = SocketOwner {
        cell_id: 20,
        generation: 1,
    };

    let cap1 = table.insert(h1, owner1).expect("insert cap1");
    let cap2 = table.insert(h2, owner2).expect("insert cap2");

    // Owner 1 can access cap1 but not cap2
    assert_eq!(table.get(cap1, owner1), Some(h1));
    assert_eq!(table.get(cap2, owner1), None);
    assert_eq!(table.get_state(cap1, owner1), Some(SocketState::Created));
    assert_eq!(table.get_state(cap2, owner1), None);
    assert!(table.is_owner(cap1, owner1));
    assert!(!table.is_owner(cap2, owner1));

    // Owner 2 can access cap2 but not cap1
    assert_eq!(table.get(cap1, owner2), None);
    assert_eq!(table.get(cap2, owner2), Some(h2));
    assert_eq!(table.get_state(cap1, owner2), None);
    assert_eq!(table.get_state(cap2, owner2), Some(SocketState::Created));
    assert!(!table.is_owner(cap1, owner2));
    assert!(table.is_owner(cap2, owner2));

    // Cross-owner listen port check
    table.set_listen_port(cap1, 8080);
    assert_eq!(table.get_listen_port(cap1, owner1), Some(8080));
    assert_eq!(table.get_listen_port(cap1, owner2), None);

    // Non-owner cannot remove socket
    assert_eq!(table.remove(cap1, owner2), None);
    assert_eq!(table.get(cap1, owner1), Some(h1));

    // Owner can remove socket
    assert_eq!(table.remove(cap1, owner1), Some(h1));
    assert_eq!(table.get(cap1, owner1), None);
}

#[test]
fn test_socket_table_generation_reuse_rejection() {
    let mut storage = [SocketStorage::EMPTY; 2];
    let mut sockets = SocketSet::new(&mut storage[..]);
    let h1 = make_test_handle(&mut sockets);

    let mut table = SocketTable::new();
    let owner1_gen1 = SocketOwner {
        cell_id: 10,
        generation: 1,
    };
    let owner1_gen2 = SocketOwner {
        cell_id: 10,
        generation: 2,
    };

    let cap1 = table.insert(h1, owner1_gen1).expect("insert cap1");

    // Generation 1 owns it
    assert_eq!(table.get(cap1, owner1_gen1), Some(h1));
    assert!(table.is_owner(cap1, owner1_gen1));

    // Recycled generation 2 of the same cell_id is rejected
    assert_eq!(table.get(cap1, owner1_gen2), None);
    assert_eq!(table.get_state(cap1, owner1_gen2), None);
    assert!(!table.is_owner(cap1, owner1_gen2));
    assert_eq!(table.remove(cap1, owner1_gen2), None);

    // Socket is still intact for generation 1
    assert_eq!(table.get(cap1, owner1_gen1), Some(h1));
}

#[test]
fn test_socket_table_udp_owner_check() {
    let mut storage = [SocketStorage::EMPTY; 2];
    let mut sockets = SocketSet::new(&mut storage[..]);
    let h = make_test_handle(&mut sockets);

    let mut table = SocketTable::new();
    let owner = SocketOwner {
        cell_id: 42,
        generation: 1,
    };
    let foreign = SocketOwner {
        cell_id: 99,
        generation: 1,
    };
    let cap = table.insert(h, owner).expect("insert");
    table.mark_udp(cap);

    assert!(table.is_udp(cap, owner));
    assert!(!table.is_udp(cap, foreign)); // foreign caller sees false
}

#[test]
fn http_owner_reserve_refuses_without_consuming_caps() {
    let mut storage = [SocketStorage::EMPTY; 2];
    let mut sockets = SocketSet::new(&mut storage[..]);
    let handle = make_test_handle(&mut sockets);
    let mut table = SocketTable::new();
    let http = SocketOwner { cell_id: 10, generation: 1 };
    let other = SocketOwner { cell_id: 20, generation: 1 };

    for _ in 0..HTTP_OWNER_BUDGET {
        table.insert(handle, http).expect("HTTP listener and 257 clients");
    }
    let next = table.next_cap_for_test();
    assert!(!table.can_insert(http));
    assert!(table.insert(handle, http).is_err());
    assert_eq!(table.next_cap_for_test(), next);
    for _ in 0..OTHER_OWNER_RESERVE {
        table.insert(handle, other).expect("other services' reserve");
    }
    assert!(!table.can_insert(other));
}

#[test]
fn owner_cleanup_preserves_other_generation() {
    let mut storage = [SocketStorage::EMPTY; 4];
    let mut sockets = SocketSet::new(&mut storage[..]);
    let old = SocketOwner { cell_id: 10, generation: 1 };
    let new = SocketOwner { cell_id: 10, generation: 2 };
    let closing_handle = make_test_handle(&mut sockets);
    let plain_handle = make_test_handle(&mut sockets);
    let new_handle = make_test_handle(&mut sockets);
    let mut table = SocketTable::new();
    let closing_cap = table.insert(closing_handle, old).expect("old owner, closing");
    let plain_cap = table.insert(plain_handle, old).expect("old owner, plain");
    let new_cap = table.insert(new_handle, new).expect("new owner");
    table.begin_graceful_close(closing_cap, 100);

    // The dead generation's plain socket is freed; the one already flushing a
    // graceful close keeps its own FIN-and-reap path; the other generation of
    // the same CellId is untouched.
    assert_eq!(table.abort_owner(old, &mut sockets, 50), (0, 1));
    assert!(table.get_unchecked(plain_cap).is_none());
    assert!(table.is_graceful_closing(closing_cap));
    assert!(!table.is_aborting(closing_cap));
    assert_eq!(table.owned_cap(old), Some(closing_cap));
    assert_eq!(table.get(new_cap, new), Some(new_handle));
    assert_eq!(table.abort_owner(old, &mut sockets, 50), (0, 0), "idempotent");

    // The graceful reaper (not owner death) finishes it.
    assert!(table.remove_internal(closing_cap).is_some());
    assert!(table.owned_cap(old).is_none());
    assert_eq!(table.get(new_cap, new), Some(new_handle));
}

fn interface() -> (Loopback, Interface) {
    let mut device = Loopback::new(Medium::Ethernet);
    let mut iface = Interface::new(
        Config::new(HardwareAddress::Ethernet(EthernetAddress([0x02, 0, 0, 0, 0, 1]))),
        &mut device,
        Instant::from_millis(0),
    );
    iface.update_ip_addrs(|addresses| {
        addresses.push(IpCidr::new(IpAddress::v4(10, 0, 0, 1), 24)).expect("address");
    });
    (device, iface)
}

/// A TCP socket with a connection tuple (SynSent), no network turn run yet.
fn connecting_handle(sockets: &mut SocketSet<'_>, iface: &mut Interface) -> SocketHandle {
    let handle = make_test_handle(sockets);
    sockets
        .get_mut::<tcp::Socket>(handle)
        .connect(iface.context(), IpEndpoint::new(IpAddress::v4(10, 0, 0, 1), 80), 49152)
        .expect("connect");
    handle
}

fn udp_handle(sockets: &mut SocketSet<'_>) -> SocketHandle {
    sockets.add(udp::Socket::new(
        udp::PacketBuffer::new(alloc::vec![udp::PacketMetadata::EMPTY; 1], alloc::vec![0u8; 64]),
        udp::PacketBuffer::new(alloc::vec![udp::PacketMetadata::EMPTY; 1], alloc::vec![0u8; 64]),
    ))
}

#[test]
fn abort_owner_treats_each_socket_kind_by_what_the_peer_must_hear() {
    let (mut device, mut iface) = interface();
    let mut sockets = SocketSet::new(alloc::vec::Vec::new());
    let owner = SocketOwner { cell_id: 10, generation: 1 };
    let other_cell = SocketOwner { cell_id: 20, generation: 1 };
    let next_generation = SocketOwner { cell_id: 10, generation: 2 };
    let mut table = SocketTable::new();

    let connected_handle = connecting_handle(&mut sockets, &mut iface);
    let connected = table.insert(connected_handle, owner).expect("connected");
    let listener_handle = make_test_handle(&mut sockets);
    sockets.get_mut::<tcp::Socket>(listener_handle).listen(80).expect("listen");
    let listener = table.insert(listener_handle, owner).expect("listener");
    let fresh_handle = make_test_handle(&mut sockets);
    let fresh = table.insert(fresh_handle, owner).expect("never connected");
    // A UDP handle must never be handed to `get_mut::<tcp::Socket>()` (it panics).
    let udp_handle = udp_handle(&mut sockets);
    let datagram = table.insert(udp_handle, owner).expect("udp");
    table.mark_udp(datagram);
    let closing_handle = connecting_handle(&mut sockets, &mut iface);
    let closing = table.insert(closing_handle, owner).expect("closing");
    table.begin_graceful_close(closing, 100);
    let foreign_handle = connecting_handle(&mut sockets, &mut iface);
    let foreign = table.insert(foreign_handle, other_cell).expect("other cell");
    let successor_handle = connecting_handle(&mut sockets, &mut iface);
    let successor = table.insert(successor_handle, next_generation).expect("next generation");
    let before = sockets.iter().count();

    // One RST owed (the connected socket); listener, never-connected and UDP freed now.
    assert_eq!(table.abort_owner(owner, &mut sockets, 1_000), (1, 3));
    assert_eq!(sockets.iter().count(), before - 3);
    for gone in [listener, fresh, datagram] {
        assert!(table.get_unchecked(gone).is_none(), "cap {gone} freed now");
        assert!(!table.is_aborting(gone));
    }

    // The connected socket is closed with its tuple kept: smoltcp owes the RST.
    assert!(table.is_aborting(connected));
    assert_eq!(table.aborting().collect::<alloc::vec::Vec<_>>(), [(connected, 1_000)]);
    let socket = sockets.get::<tcp::Socket>(connected_handle);
    assert_eq!(socket.state(), tcp::State::Closed);
    assert!(socket.remote_endpoint().is_some());

    // The graceful cap is left to its own reaper, still mid-handshake and not aborted.
    assert!(table.is_graceful_closing(closing));
    assert!(!table.is_aborting(closing));
    assert_eq!(sockets.get::<tcp::Socket>(closing_handle).state(), tcp::State::SynSent);

    // Another CellId and another generation of the same CellId are untouched.
    for (cap, handle, who) in
        [(foreign, foreign_handle, other_cell), (successor, successor_handle, next_generation)]
    {
        assert_eq!(table.get(cap, who), Some(handle));
        assert_eq!(sockets.get::<tcp::Socket>(handle).state(), tcp::State::SynSent);
        assert!(!table.is_aborting(cap));
    }

    // Idempotent: nothing more to do for the same owner.
    assert_eq!(table.abort_owner(owner, &mut sockets, 2_000), (0, 0));
    assert_eq!(table.aborting().count(), 1);

    // A network turn emits the RST and smoltcp forgets the tuple: the ledger
    // entry is satisfied and removing it returns the slot.
    let entries = table.entries.len();
    // (The frame first needs the neighbour entry, hence a few turns.)
    for turn in 1..=6 {
        iface.poll(Instant::from_millis(turn * 10), &mut device, &mut sockets);
    }
    assert!(sockets.get::<tcp::Socket>(connected_handle).remote_endpoint().is_none());
    assert!(table.remove_internal(connected).is_some());
    assert!(!table.is_aborting(connected));
    assert_eq!(table.entries.len(), entries - 1);
}

#[test]
fn remove_internal_clears_the_abort_ledger_and_the_budget_returns() {
    let (_device, mut iface) = interface();
    let mut sockets = SocketSet::new(alloc::vec::Vec::new());
    let owner = SocketOwner { cell_id: 10, generation: 1 };
    let mut table = SocketTable::new();

    // One owner may hold up to its whole HTTP budget; every one is aborted.
    for _ in 0..HTTP_OWNER_BUDGET {
        let handle = connecting_handle(&mut sockets, &mut iface);
        table.insert(handle, owner).expect("within the owner budget");
    }
    assert!(!table.can_insert(owner));
    assert_eq!(table.abort_owner(owner, &mut sockets, 7), (HTTP_OWNER_BUDGET, 0));
    // The ledger is bounded by the table, which is bounded by MAX_SOCKETS.
    assert_eq!(table.aborting().count(), HTTP_OWNER_BUDGET);
    assert!(table.aborting().count() <= MAX_SOCKETS);
    // Ledger entries still occupy their slots until finished.
    assert!(!table.can_insert(owner));

    let caps: alloc::vec::Vec<u64> = table.aborting().map(|(cap, _)| cap).collect();
    for cap in caps {
        let handle = table.remove_internal(cap).expect("ledger cap has a socket");
        sockets.remove(handle);
        assert!(!table.is_aborting(cap));
    }
    assert_eq!(table.aborting().count(), 0);
    assert!(table.owned_cap(owner).is_none());
    assert!(table.can_insert(owner), "the budget is back to baseline");
}

#[test]
fn graceful_to_abort_keeps_a_cap_in_at_most_one_ledger() {
    let (_device, mut iface) = interface();
    let mut sockets = SocketSet::new(alloc::vec::Vec::new());
    let owner = SocketOwner { cell_id: 10, generation: 1 };
    let mut table = SocketTable::new();
    let handle = connecting_handle(&mut sockets, &mut iface);
    let cap = table.insert(handle, owner).expect("cap");

    table.begin_graceful_close(cap, 100);
    assert!(table.is_graceful_closing(cap));
    table.graceful_to_abort(cap, 500);
    assert!(!table.is_graceful_closing(cap));
    assert_eq!(table.aborting().collect::<alloc::vec::Vec<_>>(), [(cap, 500)]);

    // A repeated graceful request on an aborting cap is ignored.
    table.begin_graceful_close(cap, 900);
    assert_eq!(table.graceful_closes().count(), 0);
    // The first abort deadline wins.
    table.graceful_to_abort(cap, 700);
    assert_eq!(table.aborting().collect::<alloc::vec::Vec<_>>(), [(cap, 500)]);

    // The owner reclaim skips a cap that is already aborting.
    assert_eq!(table.abort_owner(owner, &mut sockets, 1_000), (0, 0));
    assert!(table.is_aborting(cap));

    // Removing it clears both ledgers and the budget returns.
    assert!(table.remove_internal(cap).is_some());
    assert_eq!(table.aborting().count(), 0);
    assert_eq!(table.graceful_closes().count(), 0);
    assert!(table.can_insert(owner));
}
