// SPDX-License-Identifier: MPL-2.0

//! Cluster client — lets a Cell query the net-broker for remote-service lookup.
//!
//! # Protocol (raw byte framing, no postcard dependency here)
//!
//! Request byte layout (broker reads):
//!   [0] opcode: 0x01 = LookupRemote
//!   [1..3] service_id (u16 LE)
//!
//! Response byte layout:
//!   [0] status: 0x00 = Found (proxy_tid follows), 0x01 = NotFound, 0x02 = Err
//!   [1..9] proxy_tid (u64 LE, only when status == 0x00)
//!
//! The proxy_tid is the net-broker TID itself — the caller then routes all requests
//! for that service_id through the broker, which forwards them via Noise transport.
//!
//! # Usage
//! ```no_run
//! let mut cluster = ostd::cluster::ClusterRef::new();
//! if let Some(tid) = cluster.lookup_remote(9) {
//!     ostd::syscall::sys_send(tid, &my_request_bytes);
//! }
//! ```

use crate::{syscall, ViError, ViResult};
use api::syscall::service;

const OP_LOOKUP_REMOTE: u8 = 0x01;
const RESP_FOUND:       u8 = 0x00;
const RESP_NOT_FOUND:   u8 = 0x01;

const BUF_SIZE: usize = 64;

/// Client handle for the cluster net-broker.
///
/// Lazily resolves the broker TID on first use; caches it for the lifetime of
/// the struct. Re-resolves if the broker restarts (tid becomes stale → lookup fails).
pub struct ClusterRef {
    broker_tid: Option<usize>,
}

impl ClusterRef {
    pub fn new() -> Self {
        Self { broker_tid: None }
    }

    /// Returns true if a net-broker is running and reachable.
    pub fn is_available(&mut self) -> bool {
        self.resolve_tid().is_some()
    }

    /// Ask the net-broker to resolve `service_id` on any cluster peer.
    ///
    /// Returns `Some(broker_tid)` when a remote peer provides the service —
    /// the caller should send its request to `broker_tid`, which proxies it.
    /// Returns `None` when the service is not known in the cluster or the
    /// broker is unavailable.
    pub fn lookup_remote(&mut self, service_id: u16) -> Option<usize> {
        let mut resp = [0u8; BUF_SIZE];
        let n = self.raw_send_recv(
            &[OP_LOOKUP_REMOTE, service_id as u8, (service_id >> 8) as u8],
            &mut resp,
        ).ok()?;

        if n < 1 || resp[0] != RESP_FOUND || n < 9 { return None; }
        let proxy_tid = u64::from_le_bytes(resp[1..9].try_into().ok()?) as usize;
        Some(proxy_tid)
    }

    /// Send a raw payload to the broker and receive a raw response.
    ///
    /// Low-level escape hatch for P08 (gossip) / P09 (enrollment) extensions.
    /// Prefer named methods when possible.
    pub fn raw_send_recv(&mut self, req: &[u8], resp: &mut [u8]) -> ViResult<usize> {
        let tid = self.resolve_tid().ok_or(ViError::NotFound)?;
        if let syscall::SyscallResult::Err(_) = syscall::sys_send(tid, req) {
            self.broker_tid = None;
            return Err(ViError::IO);
        }
        match syscall::sys_recv(0, resp) {
            syscall::SyscallResult::Ok(sender) if sender > 0 => Ok(resp.len()),
            _ => {
                self.broker_tid = None;
                Err(ViError::IO)
            }
        }
    }

    fn resolve_tid(&mut self) -> Option<usize> {
        if let Some(tid) = self.broker_tid {
            return Some(tid);
        }
        let tid = syscall::sys_lookup_service(service::NET_BROKER)?;
        self.broker_tid = Some(tid);
        Some(tid)
    }
}

impl Default for ClusterRef {
    fn default() -> Self { Self::new() }
}
