//! Coarse per-Cell network rights, enforced before any effect.
//!
//! The kernel exports the sender Cell's reviewed net rights as
//! `CALLER_FLAG_NET_*` bits of the attested caller trailer. Every request that
//! creates network reach — a name lookup, an egress socket, an inbound port or
//! group, a raw frame — needs its right. A request that only acts on a socket
//! the caller already owns is not gated: owning one already required a gated
//! request, and the socket table rejects a foreign cap with its existing error.
//! `GetLocalIp` stays ungated (it discloses only the local address).
//!
//! A denial is `NetResponse::Err(NET_ERR_DENIED)` and nothing else: the runtime
//! answers it before the owner watch, both intercepts and dispatch, so no socket,
//! DNS query, owner watch or frame exists because of a denied request.

use api::caller_identity::{
    CALLER_FLAG_NET_CONNECT, CALLER_FLAG_NET_L2, CALLER_FLAG_NET_LISTEN, CALLER_FLAG_NET_RESOLVE,
};
use api::ipc::{self, NetRequest, NetResponse, NET_ERR_DENIED};

use crate::tls_wire::{parse_raw_tls_request, RawTlsRequest, TLS_CLOSE_OP};

/// One coarse network right.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Right {
    /// D: name resolution.
    Resolve,
    /// N: egress sockets.
    Connect,
    /// Nl: inbound ports and multicast groups.
    Listen,
    /// L2: raw Ethernet frames.
    L2,
}

impl Right {
    /// The attested caller flag that carries this right.
    pub(crate) const fn caller_flag(self) -> u32 {
        match self {
            Self::Resolve => CALLER_FLAG_NET_RESOLVE,
            Self::Connect => CALLER_FLAG_NET_CONNECT,
            Self::Listen => CALLER_FLAG_NET_LISTEN,
            Self::L2 => CALLER_FLAG_NET_L2,
        }
    }
}

/// The right a typed request needs, or `None` when it is ungated.
///
/// No `_` arm: a new `NetRequest` variant does not compile until it is
/// classified here.
pub(crate) fn required_right(request: &NetRequest<'_>) -> Option<Right> {
    match request {
        NetRequest::Resolve { .. } => Some(Right::Resolve),
        NetRequest::TcpConnect { .. } | NetRequest::UdpCreate | NetRequest::UdpSend { .. } => {
            Some(Right::Connect)
        }
        NetRequest::TcpListen { .. }
        | NetRequest::UdpBind { .. }
        | NetRequest::MulticastJoin { .. } => Some(Right::Listen),
        NetRequest::L2Send { .. } | NetRequest::L2Recv { .. } => Some(Right::L2),
        NetRequest::TcpSend { .. }
        | NetRequest::TcpRecv { .. }
        | NetRequest::TcpClose { .. }
        | NetRequest::TcpAccept { .. }
        | NetRequest::UdpRecv { .. }
        | NetRequest::SocketState { .. }
        | NetRequest::GetLocalIp
        | NetRequest::MulticastLeave { .. }
        | NetRequest::TcpReady { .. }
        | NetRequest::TcpRecvReady { .. }
        | NetRequest::TcpSendReady { .. }
        | NetRequest::TcpCloseGraceful { .. } => None,
    }
}

/// The right a legacy raw TLS request needs, or `None` when it is ungated.
///
/// `Connect` opens an outbound TCP socket to any address, so it is egress like
/// `TcpConnect`; the rest act on a TLS cap the caller already owns.
pub(crate) fn raw_tls_required_right(request: &RawTlsRequest<'_>) -> Option<Right> {
    match request {
        RawTlsRequest::Connect { .. } => Some(Right::Connect),
        RawTlsRequest::Close { .. } | RawTlsRequest::Send { .. } | RawTlsRequest::Recv { .. } => {
            None
        }
    }
}

/// How a denied request is answered: in the wire format its caller reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Denial {
    /// `NetResponse::Err(NET_ERR_DENIED)`.
    Typed,
    /// Raw TLS connect has no error code on its wire: cap `0` is its existing
    /// "connect failed" reply.
    RawTlsConnect,
}

/// Bytes enough for either denial reply.
pub(crate) const DENIAL_REPLY_LEN: usize = 8;

impl Denial {
    /// Encode the denial reply into `out`.
    pub(crate) fn reply(self, out: &mut [u8; DENIAL_REPLY_LEN]) -> &[u8] {
        match self {
            Self::Typed => match ipc::encode(&NetResponse::Err(NET_ERR_DENIED), out) {
                Ok(bytes) => bytes,
                // Two postcard bytes always fit; an empty reply is still a refusal.
                Err(_) => &[],
            },
            Self::RawTlsConnect => {
                *out = 0u64.to_le_bytes();
                &out[..]
            }
        }
    }
}

/// Admit or deny one attested request by the caller's flags.
///
/// Classification mirrors dispatch exactly: a buffer that decodes as a
/// `NetRequest` is typed (both intercepts and `handle_request` decode it the same
/// way); otherwise a first byte at or above `TLS_CLOSE_OP` goes to the raw TLS
/// handler. Anything else reaches no handler and has no effect to gate.
pub(crate) fn admit(buffer: &[u8], flags: u32) -> Result<(), Denial> {
    let (right, denial) = match ipc::decode::<NetRequest<'_>>(buffer) {
        Ok(request) => (required_right(&request), Denial::Typed),
        Err(_) if buffer.first().copied().unwrap_or(0) >= TLS_CLOSE_OP => {
            match parse_raw_tls_request(buffer) {
                Ok(request) => (raw_tls_required_right(&request), Denial::RawTlsConnect),
                Err(_) => (None, Denial::Typed),
            }
        }
        Err(_) => (None, Denial::Typed),
    };
    match right {
        Some(right) if flags & right.caller_flag() == 0 => Err(denial),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
