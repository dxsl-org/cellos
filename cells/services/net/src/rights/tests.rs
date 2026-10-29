use super::*;
use api::caller_identity::CALLER_FLAG_NET_RIGHTS_MASK;
use api::ipc::IPC_BUF_SIZE;

use crate::tls_wire::{TLS_CONNECT_OP, TLS_RECV_OP, TLS_SEND_OP};

/// Every `NetRequest` variant with its wire discriminant and required right.
fn census() -> [(NetRequest<'static>, u8, Option<Right>); 21] {
    [
        (NetRequest::TcpConnect { addr: [10, 0, 2, 2], port: 80 }, 0, Some(Right::Connect)),
        (NetRequest::TcpSend { cap_id: 1, data: b"x" }, 1, None),
        (NetRequest::TcpRecv { cap_id: 1, buf_len: 8 }, 2, None),
        (NetRequest::TcpClose { cap_id: 1 }, 3, None),
        (NetRequest::TcpListen { port: 80 }, 4, Some(Right::Listen)),
        (NetRequest::TcpAccept { cap_id: 1 }, 5, None),
        (NetRequest::UdpCreate, 6, Some(Right::Connect)),
        (
            NetRequest::UdpSend { cap_id: 1, addr: [10, 0, 2, 2], port: 9, data: b"x" },
            7,
            Some(Right::Connect),
        ),
        (NetRequest::UdpRecv { cap_id: 1, buf_len: 8 }, 8, None),
        (NetRequest::Resolve { hostname: "example.com" }, 9, Some(Right::Resolve)),
        (NetRequest::SocketState { cap_id: 1 }, 10, None),
        (NetRequest::UdpBind { cap_id: 1, port: 5000 }, 11, Some(Right::Listen)),
        (NetRequest::GetLocalIp, 12, None),
        (
            NetRequest::MulticastJoin { cap_id: 0, group: [239, 0, 0, 1] },
            13,
            Some(Right::Listen),
        ),
        (NetRequest::MulticastLeave { cap_id: 0, group: [239, 0, 0, 1] }, 14, None),
        (NetRequest::L2Send { data: &[0u8; 60] }, 15, Some(Right::L2)),
        (NetRequest::L2Recv { guest_mac: [0x52, 0x54, 0, 0, 0, 1] }, 16, Some(Right::L2)),
        (NetRequest::TcpReady { interests: &[], cursor: 0, wait: false }, 17, None),
        (NetRequest::TcpRecvReady { cap_id: 1, buf_len: 8 }, 18, None),
        (NetRequest::TcpSendReady { cap_id: 1, data: b"x" }, 19, None),
        (NetRequest::TcpCloseGraceful { cap_id: 1 }, 20, None),
    ]
}

const ALL_RIGHTS: [Right; 4] = [Right::Resolve, Right::Connect, Right::Listen, Right::L2];

#[test]
fn every_request_variant_has_the_annex_right() {
    for (request, discriminant, right) in census() {
        let mut buf = [0u8; IPC_BUF_SIZE];
        let bytes = ipc::encode(&request, &mut buf).expect("encode");
        assert_eq!(bytes[0], discriminant, "{request:?} moved on the wire");
        assert_eq!(required_right(&request), right, "{request:?}");
    }
}

#[test]
fn rights_map_to_distinct_caller_flags_inside_the_mask() {
    let mut seen = 0;
    for right in ALL_RIGHTS {
        let flag = right.caller_flag();
        assert_eq!(flag.count_ones(), 1);
        assert_eq!(seen & flag, 0, "{right:?} shares a flag");
        seen |= flag;
    }
    assert_eq!(seen, CALLER_FLAG_NET_RIGHTS_MASK);
}

#[test]
fn admission_needs_exactly_the_request_right() {
    for (request, _, right) in census() {
        let mut buf = [0u8; IPC_BUF_SIZE];
        let bytes = ipc::encode(&request, &mut buf).expect("encode").to_vec();
        match right {
            None => {
                assert_eq!(admit(&bytes, 0), Ok(()), "{request:?} is ungated");
            }
            Some(right) => {
                let flag = right.caller_flag();
                assert_eq!(admit(&bytes, 0), Err(Denial::Typed), "{request:?}");
                // Every other right, and every reserved bit, is not this one.
                assert_eq!(admit(&bytes, !flag), Err(Denial::Typed), "{request:?}");
                assert_eq!(admit(&bytes, flag), Ok(()), "{request:?}");
                assert_eq!(admit(&bytes, CALLER_FLAG_NET_RIGHTS_MASK), Ok(()), "{request:?}");
            }
        }
    }
}

#[test]
fn typed_denial_is_err_denied() {
    let mut out = [0u8; DENIAL_REPLY_LEN];
    let reply = Denial::Typed.reply(&mut out);
    match ipc::decode::<NetResponse<'_>>(reply) {
        Ok(NetResponse::Err(code)) => assert_eq!(code, NET_ERR_DENIED),
        other => panic!("denial decoded as {other:?}"),
    }
    assert_eq!(NET_ERR_DENIED, 0xFC);
}

fn raw_tls_connect() -> [u8; 21] {
    let mut request = [0u8; 21];
    request[0] = TLS_CONNECT_OP;
    request[9..13].copy_from_slice(&[10, 0, 2, 2]);
    request[13..15].copy_from_slice(&443u16.to_le_bytes());
    request[15..17].copy_from_slice(&4u16.to_le_bytes());
    request[17..].copy_from_slice(b"host");
    request
}

#[test]
fn raw_tls_connect_is_egress_and_owned_cap_ops_are_not() {
    let connect = raw_tls_connect();
    assert_eq!(admit(&connect, 0), Err(Denial::RawTlsConnect));
    assert_eq!(
        admit(&connect, CALLER_FLAG_NET_RIGHTS_MASK & !CALLER_FLAG_NET_CONNECT),
        Err(Denial::RawTlsConnect)
    );
    assert_eq!(admit(&connect, CALLER_FLAG_NET_CONNECT), Ok(()));

    let mut out = [0u8; DENIAL_REPLY_LEN];
    assert_eq!(Denial::RawTlsConnect.reply(&mut out), &0u64.to_le_bytes());

    let mut close = [0u8; 9];
    close[0] = TLS_CLOSE_OP;
    let mut send = [0u8; 12];
    send[0] = TLS_SEND_OP;
    send[9..11].copy_from_slice(&1u16.to_le_bytes());
    let mut recv = [0u8; 13];
    recv[0] = TLS_RECV_OP;
    for raw in [&close[..], &send[..], &recv[..]] {
        let parsed = parse_raw_tls_request(raw).expect("parse");
        assert_eq!(raw_tls_required_right(&parsed), None);
        assert_eq!(admit(raw, 0), Ok(()));
    }
}

#[test]
fn bytes_no_handler_accepts_have_nothing_to_gate() {
    // Neither a `NetRequest` nor a parseable raw TLS request: dispatch drops it.
    assert_eq!(admit(&[TLS_CONNECT_OP], 0), Ok(()));
    assert_eq!(admit(&[], 0), Ok(()));
}
