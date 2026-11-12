use super::*;

const HOST: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
const GUEST_A: [u8; 6] = [0x52, 0x54, 0x00, 0xAA, 0xBB, 0xCC];
const GUEST_B: [u8; 6] = [0x52, 0x54, 0x00, 0xAA, 0xBB, 0xCD];
const PEER: [u8; 6] = [0x52, 0x55, 0x0A, 0x00, 0x02, 0x02];
const A: SocketOwner = SocketOwner {
    cell_id: 7,
    generation: 1,
};
const B: SocketOwner = SocketOwner {
    cell_id: 8,
    generation: 1,
};

fn frame(dst: [u8; 6], src: [u8; 6]) -> Vec<u8> {
    let mut f = alloc::vec![0u8; 60];
    f[..6].copy_from_slice(&dst);
    f[6..12].copy_from_slice(&src);
    f[12..14].copy_from_slice(&[0x08, 0x00]);
    f
}

fn two_guests() -> Bridge {
    let mut bridge = Bridge::default();
    bridge.open(A, GUEST_A, HOST).expect("A opens");
    bridge.open(B, GUEST_B, HOST).expect("B opens");
    bridge
}

#[test]
fn unicast_for_the_host_stays_with_the_host() {
    let mut bridge = two_guests();
    assert!(bridge.route(&frame(HOST, PEER)));
    assert!(bridge.pop(A).is_none());
    assert!(bridge.pop(B).is_none());
}

#[test]
fn unicast_for_a_guest_reaches_only_that_guest() {
    let mut bridge = two_guests();
    assert!(
        !bridge.route(&frame(GUEST_A, PEER)),
        "the host must not get it"
    );
    assert_eq!(&bridge.pop(A).expect("A's frame")[..6], &GUEST_A);
    assert!(bridge.pop(B).is_none(), "B must not see A's unicast");
}

#[test]
fn broadcast_and_multicast_reach_the_host_and_every_guest() {
    let mut bridge = two_guests();
    for dst in [[0xFF; 6], [0x01, 0x00, 0x5E, 0x00, 0x00, 0xFB]] {
        assert!(bridge.route(&frame(dst, PEER)));
        assert!(bridge.pop(A).is_some());
        assert!(bridge.pop(B).is_some());
    }
}

#[test]
fn a_guest_cannot_send_as_the_host_or_another_guest() {
    let mut bridge = two_guests();
    assert_eq!(
        bridge.admit_tx(A, &frame([0xFF; 6], HOST), HOST),
        Err(PortError::AddressInUse)
    );
    assert_eq!(
        bridge.admit_tx(A, &frame([0xFF; 6], GUEST_B), HOST),
        Err(PortError::AddressInUse)
    );
    assert_eq!(
        bridge.admit_tx(A, &frame([0xFF; 6], [0x01, 0, 0, 0, 0, 1]), HOST),
        Err(PortError::NotStation)
    );
    assert_eq!(
        bridge.mac(A),
        Some(GUEST_A),
        "a refused frame teaches nothing"
    );
    // Host unicast still goes to the host, B's unicast still to B.
    assert!(bridge.route(&frame(HOST, PEER)));
    assert!(!bridge.route(&frame(GUEST_B, PEER)));
    assert!(bridge.pop(A).is_none());
}

#[test]
fn a_port_cannot_open_at_a_taken_address() {
    let mut bridge = Bridge::default();
    assert_eq!(bridge.open(A, HOST, HOST), Err(PortError::AddressInUse));
    bridge.open(A, GUEST_A, HOST).expect("A opens");
    assert_eq!(bridge.open(B, GUEST_A, HOST), Err(PortError::AddressInUse));
}

/// The board's guest sent from an address other than the one its hypervisor
/// named; replies must follow the address it sends from, and the next poll
/// naming the old address must not undo that.
#[test]
fn replies_follow_the_address_the_guest_sends_from() {
    let mut bridge = Bridge::default();
    bridge.open(A, GUEST_A, HOST).expect("A opens");
    let actual = [0x52, 0x00, 0x00, 0x00, 0xBB, 0x00];
    bridge
        .admit_tx(A, &frame([0xFF; 6], actual), HOST)
        .expect("own new address");
    bridge.open(A, GUEST_A, HOST).expect("poll");
    assert!(!bridge.route(&frame(actual, PEER)));
    assert!(bridge.pop(A).is_some());
    assert!(
        bridge.route(&frame(GUEST_A, PEER)),
        "the old address is free"
    );
}

#[test]
fn a_released_port_frees_its_address_and_queue() {
    let mut bridge = two_guests();
    assert!(!bridge.route(&frame(GUEST_A, PEER)));
    assert!(bridge.release(A));
    assert!(bridge.pop(A).is_none());
    assert!(bridge.route(&frame(GUEST_A, PEER)), "nobody owns it now");
    bridge.open(B, GUEST_B, HOST).expect("B unaffected");
    let reopened = SocketOwner {
        cell_id: 7,
        generation: 2,
    };
    bridge
        .open(reopened, GUEST_A, HOST)
        .expect("address reusable");
}
