use super::*;
use smoltcp::iface::{SocketSet, SocketStorage};
use smoltcp::socket::tcp;

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
    let old_handle = make_test_handle(&mut sockets);
    let new_handle = make_test_handle(&mut sockets);
    let mut table = SocketTable::new();
    let old_cap = table.insert(old_handle, old).expect("old owner");
    let new_cap = table.insert(new_handle, new).expect("new owner");
    table.begin_graceful_close(old_cap, 100);
    table.remove_owner(old, &mut sockets);
    assert!(table.owned_cap(old).is_none());
    assert!(!table.is_graceful_closing(old_cap));
    assert_eq!(table.get(new_cap, new), Some(new_handle));
}
