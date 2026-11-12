// SPDX-License-Identifier: Apache-2.0
//! Net service error bytes carried in `NetResponse::Err(u8)`; the requests are
//! the postcard `NetRequest`/`NetResponse` in `ipc.rs`, which also owns the
//! readiness frames re-exported here.

pub use crate::services::ipc::{
    net_ready, NET_READY_FRAME_BYTES, NET_READY_OPCODE, NET_READY_RECORD_BYTES, REACTOR_WAKE_OPCODE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NetError {
    /// The port is already bound.
    AddrInUse = 0xFB,
    /// The caller lacks the right for the request.
    Denied = 0xFC,
    /// A queue or slot is full, or the wait was refused.
    Busy = 0xFD,
    /// Not ready: no connection to accept yet, or the socket is not established.
    NotReady = 0xFE,
    /// Generic failure. On `TcpRecv` the same byte is [`TCP_RECV_EOF`].
    Failure = 0xFF,
}

/// `TcpRecv` end of stream. Shares its byte with [`NetError::Failure`]; only
/// the request kind tells them apart.
pub const TCP_RECV_EOF: u8 = 0xFF;

/// `NetRequest` postcard variants stay below this index: the raw TLS frames
/// are routed by first byte `>=` [`super::tls::CLOSE`] when postcard decode
/// fails, so variant 0x15 would be taken for `TLS_CLOSE`.
pub const NET_REQUEST_VARIANT_LIMIT: u8 = super::tls::CLOSE;
