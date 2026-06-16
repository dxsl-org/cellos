// SPDX-License-Identifier: MPL-2.0

//! Network service client — ergonomic TCP/UDP access.

extern crate alloc;

use alloc::vec::Vec;
use api::ipc::{IPC_BUF_SIZE, NetRequest, NetResponse};
use crate::{ViError, ViResult};
use crate::service::NetRef;
use super::vierr_from_code;

/// A TCP/UDP socket handle returned by [`NetClient::tcp_connect`].
pub type SocketId = u32;

/// Ergonomic client for the network service.
///
/// Wraps [`NetRef`] and hides request construction + postcard encoding.
pub struct NetClient {
    svc: NetRef,
}

impl NetClient {
    /// Create a new unresolved client. Resolution is lazy (first call).
    pub fn new() -> Self {
        Self { svc: NetRef::new() }
    }

    /// Open a TCP connection to `addr:port`.
    ///
    /// Returns a `SocketId` on success.  The socket is owned by the net service;
    /// call [`tcp_close`][Self::tcp_close] when done.
    pub fn tcp_connect(&mut self, addr: [u8; 4], port: u16) -> ViResult<SocketId> {
        let req = NetRequest::TcpConnect { addr, port };
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<NetRequest, NetResponse>(&req, &mut resp_buf)? {
            NetResponse::CapId(id) => Ok(id),
            NetResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Send `data` over an established TCP socket.
    pub fn tcp_send(&mut self, id: SocketId, data: &[u8]) -> ViResult<()> {
        let req = NetRequest::TcpSend { cap_id: id, data };
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<NetRequest, NetResponse>(&req, &mut resp_buf)? {
            NetResponse::Ok => Ok(()),
            NetResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Receive up to `buf_len` bytes from an established TCP socket.
    pub fn tcp_recv(&mut self, id: SocketId, buf_len: u32) -> ViResult<Vec<u8>> {
        let req = NetRequest::TcpRecv { cap_id: id, buf_len };
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<NetRequest, NetResponse>(&req, &mut resp_buf)? {
            NetResponse::Data(data) => Ok(data.to_vec()),
            NetResponse::Ok => Ok(alloc::vec![]),
            NetResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Close a TCP connection.
    pub fn tcp_close(&mut self, id: SocketId) -> ViResult<()> {
        let req = NetRequest::TcpClose { cap_id: id };
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<NetRequest, NetResponse>(&req, &mut resp_buf)? {
            NetResponse::Ok => Ok(()),
            NetResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Resolve a hostname to an IPv4 address.
    pub fn dns_lookup(&mut self, hostname: &str) -> ViResult<[u8; 4]> {
        let req = NetRequest::Resolve { hostname };
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<NetRequest, NetResponse>(&req, &mut resp_buf)? {
            NetResponse::Addr(addr) => Ok(addr),
            NetResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Return the DHCP-assigned local IPv4 address.
    pub fn local_ip(&mut self) -> ViResult<[u8; 4]> {
        let req = NetRequest::GetLocalIp;
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self.svc.call::<NetRequest, NetResponse>(&req, &mut resp_buf)? {
            NetResponse::Addr(addr) => Ok(addr),
            NetResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }
}

impl Default for NetClient {
    fn default() -> Self { Self::new() }
}
