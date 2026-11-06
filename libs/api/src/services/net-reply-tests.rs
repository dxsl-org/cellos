//! Host tests for the POSIX net reply gate (audit H-32).

use super::admit_net_reply;
use crate::ipc::{encode, NetResponse, IPC_BUF_SIZE};
use crate::service_binding::ServiceBinding;

/// Above `IPC_BUF_SIZE`: the old `&buf[..tid]` slice panicked on this tid.
const NET_TID: u64 = 4_101;

fn net() -> ServiceBinding {
    ServiceBinding::new(NET_TID, 9, 3)
}

/// A receive buffer as the kernel leaves it: the frame, then zeroes.
fn received(reply: &NetResponse<'_>) -> [u8; IPC_BUF_SIZE] {
    let mut buf = [0u8; IPC_BUF_SIZE];
    encode(reply, &mut buf).expect("reply fits the IPC buffer");
    buf
}

#[test]
fn decodes_the_providers_reply_from_the_whole_buffer() {
    assert!(NET_TID as usize > IPC_BUF_SIZE);
    let buf = received(&NetResponse::CapId(7));
    assert!(matches!(
        admit_net_reply(&net(), NET_TID as isize, &buf),
        Some(NetResponse::CapId(7))
    ));
}

#[test]
fn a_small_provider_tid_does_not_truncate_the_frame() {
    // tid 2 used to cut a `Data` frame to its first two bytes.
    let small = ServiceBinding::new(2, 9, 3);
    let payload = [0xAB_u8; 40];
    let buf = received(&NetResponse::Data(&payload));
    match admit_net_reply(&small, 2, &buf) {
        Some(NetResponse::Data(data)) => assert_eq!(data, &payload[..]),
        other => panic!("expected the full Data payload, got {other:?}"),
    }
}

#[test]
fn refuses_a_reply_forged_by_any_other_sender() {
    let buf = received(&NetResponse::Addr([6, 6, 6, 6]));
    assert!(admit_net_reply(&net(), NET_TID as isize + 1, &buf).is_none());
    assert!(admit_net_reply(&net(), 1, &buf).is_none());
}

#[test]
fn refuses_a_failed_recv() {
    let buf = received(&NetResponse::Ok);
    assert!(admit_net_reply(&net(), 0, &buf).is_none());
    assert!(admit_net_reply(&net(), -1, &buf).is_none());
}

#[test]
fn refuses_everything_without_a_live_provider() {
    // A dead or unregistered NET binding must never mean "accept anyone",
    // including a sender whose tid matches the stale record.
    let buf = received(&NetResponse::Ok);
    let dead = ServiceBinding::new(NET_TID, 0, 3);
    assert!(admit_net_reply(&dead, NET_TID as isize, &buf).is_none());
    let unregistered = ServiceBinding::new(0, 0, 0);
    assert!(admit_net_reply(&unregistered, 0, &buf).is_none());
}

#[test]
fn refuses_a_frame_that_does_not_decode() {
    let buf = [0xFFu8; IPC_BUF_SIZE];
    assert!(admit_net_reply(&net(), NET_TID as isize, &buf).is_none());
}
