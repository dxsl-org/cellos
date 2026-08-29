//! Unit tests for bounded copied IPC wire message.

use super::ipc_wire::*;

#[test]
fn wire_message_bounds() {
    let header = IpcWireHeader {
        sender_tid: 1,
        sender_cell_id: 10,
        sender_generation: 1,
        delivery_id: 7,
    };
    let small_payload = [0xabu8; 128];
    let msg =
        IpcWireMessage::try_new(header, &small_payload).expect("should allocate small payload");
    assert_eq!(msg.len(), 128);
    assert_eq!(msg.as_slice(), &small_payload[..]);
    assert_eq!(msg.header.sender_tid, 1);
    assert_eq!(msg.header.sender_cell_id, 10);
    assert_eq!(msg.header.sender_generation, 1);
    assert_eq!(msg.header.delivery_id, 7);

    let empty = IpcWireMessage::try_new(header, &[]).expect("should allow empty payload");
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);

    // Bounded max payload
    let max_payload = alloc::vec![0xcd; MAX_IPC_WIRE_PAYLOAD];
    let msg_max = IpcWireMessage::try_new(header, &max_payload).expect("should allow max payload");
    assert_eq!(msg_max.len(), MAX_IPC_WIRE_PAYLOAD);

    // Exceeding payload must fail
    let overlarge = alloc::vec![0xef; MAX_IPC_WIRE_PAYLOAD + 1];
    assert!(IpcWireMessage::try_new(header, &overlarge).is_err());
}
