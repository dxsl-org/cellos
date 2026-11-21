//! 07 S-9: neither legacy replies nor requests queued behind a TLS operation
//! may grow without bound. On the host the legacy `TrySend` never delivers, so
//! every legacy reply takes the retention path.
use super::*;
use smoltcp::iface::SocketStorage;
use smoltcp::socket::udp;

fn owner(cell_id: u64) -> SocketOwner {
    SocketOwner {
        cell_id,
        generation: 1,
    }
}

fn udp_socket() -> udp::Socket<'static> {
    udp::Socket::new(
        udp::PacketBuffer::new(
            alloc::vec![udp::PacketMetadata::EMPTY; 1],
            alloc::vec![0u8; 16],
        ),
        udp::PacketBuffer::new(
            alloc::vec![udp::PacketMetadata::EMPTY; 1],
            alloc::vec![0u8; 16],
        ),
    )
}

#[test]
fn legacy_replies_stay_bounded_per_owner_and_in_total() {
    let mut storage = [SocketStorage::EMPTY; 1];
    let mut sockets = SocketSet::new(&mut storage[..]);
    let handle = sockets.add(udp_socket());
    let mut table = SocketTable::new();
    let flooder = owner(10);
    let cap = table.insert(handle, flooder).expect("cap");
    table.mark_udp(cap);
    let mut tls_table = BTreeMap::new();
    let mut pending = TlsPending::default();

    // Raw-TLS sends on the flooder's own non-TLS cap: each is answered SEND_ERROR.
    for sender in 0..10_000 {
        pending.enqueue(
            cap,
            flooder,
            ReplyTo::Legacy(1_000 + sender),
            Request::Send(alloc::vec![0u8; 64]),
            &mut tls_table,
            &table,
        );
    }
    assert_eq!(pending.replies.len(), MAX_RETAINED_PER_OWNER);

    // Many owners together cannot pass the global bound either.
    for cell in 0..1_000 {
        pending.enqueue(
            cap,
            owner(100 + cell),
            ReplyTo::Legacy(50_000 + cell as usize),
            Request::Recv(64),
            &mut tls_table,
            &table,
        );
    }
    assert_eq!(pending.replies.len(), MAX_RETAINED_REPLIES);
}

#[test]
fn requests_queued_behind_an_operation_stay_bounded() {
    let mut storage = [SocketStorage::EMPTY; 1];
    let mut sockets = SocketSet::new(&mut storage[..]);
    let handle = sockets.add(udp_socket());
    let mut table = SocketTable::new();
    let holder = owner(10);
    let cap = table.insert(handle, holder).expect("cap");
    let mut tls_table = BTreeMap::new();
    let mut pending = TlsPending::default();
    pending.connect(
        cap,
        handle,
        holder,
        ReplyTo::Legacy(1),
        String::from("example.test"),
    );

    for sender in 0..10_000 {
        pending.enqueue(
            cap,
            holder,
            ReplyTo::Legacy(1_000 + sender),
            Request::Send(alloc::vec![0u8; 64]),
            &mut tls_table,
            &table,
        );
    }
    assert_eq!(pending.operations[&cap].queue.len(), MAX_QUEUED_PER_OP);
    assert!(pending.replies.len() <= MAX_RETAINED_PER_OWNER);
}
