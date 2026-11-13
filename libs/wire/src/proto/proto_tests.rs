// SPDX-License-Identifier: Apache-2.0
//! Byte pins: each value equals the literal it replaced (Phase 12 CP1 W1).
//! A failing assertion here is a wire break, never a test to update.

use super::*;

#[test]
fn app_envelope_bytes() {
    use app_envelope::*;
    assert_eq!(
        [
            MAGIC,
            KIND_MESSAGE,
            KIND_SNAPSHOT,
            KIND_RESTORE,
            KIND_CAP_REVOKED,
            KIND_SHUTDOWN
        ],
        [0xAC, 0x00, 0xF0, 0xF1, 0xF2, 0xFF]
    );
    assert_eq!((SNAPSHOT_LEN, RESTORE_LEN, CAP_REVOKED_LEN), (10, 66, 6));
    assert_eq!((SHUTDOWN_WATCHDOG, SHUTDOWN_PARENT_DIED), (1, 2));
}

#[test]
fn supervisor_bytes() {
    use supervisor::*;
    assert_eq!([OP_HOTSWAP, OP_SNAPSHOT, OP_STATUS, OP_POWER], [1, 2, 3, 4]);
    assert_eq!(
        [
            PHASE_REFUSED,
            PHASE_SNAPSHOT,
            PHASE_POWER,
            PHASE_HOTSWAP_DONE,
            PHASE_HOTSWAP_ERROR
        ],
        [0, 1, 2, 6, 0xFF]
    );
    assert_eq!(
        [
            STATUS_OK,
            STATUS_UNAVAILABLE,
            STATUS_REJECTED_CALLER,
            STATUS_SERVICE_NOT_FOUND,
            STATUS_INVALID_REQUEST
        ],
        [0x00, 0x01, 0xFD, 0xFE, 0xFF]
    );
    assert_eq!(
        (HOTSWAP_REQUEST_LEN, SNAPSHOT_REQUEST_LEN, POWER_REQUEST_LEN),
        (193, 1, 2)
    );
    assert_eq!((POWER_ACTION_REBOOT, POWER_ACTION_POWEROFF), (1, 2));
    assert_eq!(POWER_BUTTON_REQUEST, [0xAC, 0x00, 0x04, 0x02]);
    assert_eq!((OWNER_EXITED_NOTICE, OWNER_EXITED_RELAY), (0xE1, 0xE2));
    for code in 0..=8u8 {
        assert_eq!(HotswapError::from_u8(code).map(|e| e as u8), Some(code));
    }
    assert_eq!(HotswapError::from_u8(9), None);
}

#[test]
fn compositor_bytes() {
    assert_eq!((compositor::REPLY_FALSE, compositor::REPLY_TRUE), (0, 1));
    assert_eq!(
        (compositor::DESTROY_OK, compositor::DESTROY_REFUSED),
        (0, 1)
    );
}

#[test]
fn config_bytes() {
    assert_eq!(config::ConfigError::Denied as u8, 0xFC);
    assert_eq!(config::ConfigError::Malformed as u8, 0xFF);
}

#[test]
fn input_bytes() {
    use input::*;
    assert_eq!(
        [EV_KEY, EV_REL, EV_ABS, EV_ASCII, EV_DEVICE],
        [0, 1, 2, 4, 5]
    );
    assert_eq!((PRODUCER_FRAME_LEN, DEVICE_FRAME_LEN), (9, 14));
    assert_eq!(
        (INPUT_EVENT_OPCODE, OP_HID_DEVICE_REMOVED, OP_SET_LEDS),
        (0x10, 0x07, 0x03)
    );
}

#[test]
fn net_bytes() {
    use net::NetError::*;
    assert_eq!(
        [
            AddrInUse as u8,
            Denied as u8,
            Busy as u8,
            NotReady as u8,
            Failure as u8
        ],
        [0xFB, 0xFC, 0xFD, 0xFE, 0xFF]
    );
    assert_eq!(net::TCP_RECV_EOF, 0xFF);
}

#[test]
fn net_request_stays_below_tls_routing() {
    use crate::ipc::NetRequest;
    // The last postcard variant: one more would collide with `tls::CLOSE`.
    let mut bytes = [0u8; 16];
    let bytes =
        postcard::to_slice(&NetRequest::TcpCloseGraceful { cap_id: 0 }, &mut bytes).unwrap();
    assert!(
        bytes[0] < net::NET_REQUEST_VARIANT_LIMIT,
        "variant {}",
        bytes[0]
    );
}

#[test]
fn tls_bytes() {
    use tls::*;
    assert_eq!([CLOSE, CONNECT, SEND, RECV], [0x15, 0x30, 0x31, 0x32]);
    assert_eq!(
        (
            HEADER_LEN,
            CONNECT_HEADER_LEN,
            SEND_HEADER_LEN,
            RECV_REQUEST_LEN
        ),
        (9, 17, 11, 13)
    );
    assert_eq!((MAX_HOSTNAME, MAX_SEND, MAX_RECV_DATA), (495, 501, 4094));
    assert_eq!(
        (SEND_ERROR, RECV_ERROR, CLOSE_OK, CLOSE_FAILED),
        (u32::MAX, u16::MAX, 0, 0xFF)
    );
}

#[test]
fn nic_bytes() {
    use nic::*;
    assert_eq!([OP_TX, OP_RX, OP_GETMAC], [0, 1, 2]);
    assert_eq!(
        [
            NicStatus::Ok as u8,
            NicStatus::Failed as u8,
            NicStatus::NotReady as u8
        ],
        [0, 1, 2]
    );
    assert_eq!((MIN_FRAME, MAC_LEN), (14, 6));
}

#[test]
fn vfs_bytes() {
    use vfs::VfsError::*;
    let all = [Io, Quota, Denied, Handle, Unsupported, Malformed];
    assert_eq!(all.map(|e| e as u8), [1, 2, 3, 4, 6, 0xFF]);
    for e in all {
        assert_eq!(vfs::VfsError::from_u8(e as u8), Some(e));
    }
    assert_eq!(vfs::VfsError::from_u8(5), None);
}
