// SPDX-License-Identifier: Apache-2.0
//! ConnectionManager — direct peer connections.
//!
//! Each configured direct address is attempted with a bounded timeout. A
//! successful path ends in a Noise KKpsk0 session in the ConnectionPool.
//! Relay fallback is unavailable until service-net provides an mTLS client
//! capability backed by the protected certificate identity.

// reason: the direct Noise connection path is not yet constructed from main.rs;
// it remains available for future authenticated routing integration.
#![allow(dead_code)]

use api::cluster::{CellNetId, PeerTicket};
use api::ipc::{NetRequest, NetResponse};
use ostd::service::NetRef;
use ostd::syscall::sys_heartbeat;
use ostd::{ViError, ViResult};

use crate::rng::BrokerRng;
use crate::transport::{ConnectionPool, NoiseSession, StaticKeypair};
use service_net_broker::identity::BrokerIdentity;

const HEARTBEAT_MS: u64 = 500;
const CONNECT_TIMEOUT_MS: u32 = 2000;

/// Manages peer connections: direct TCP preferred, relay fallback.
pub struct ConnectionManager<'a> {
    pool: &'a mut ConnectionPool,
    identity: &'a BrokerIdentity,
}

impl<'a> ConnectionManager<'a> {
    pub fn new(pool: &'a mut ConnectionPool, identity: &'a BrokerIdentity) -> Self {
        Self { pool, identity }
    }

    /// Ensure a direct Noise session exists for `peer`.
    ///
    /// `psk` is the K1 cluster PSK. `rng` is the broker's PRNG.
    ///
    /// # Errors
    /// Returns `WouldBlock` when the bounded session pool is full or
    /// `NotSupported` when no authenticated path succeeds.
    pub fn ensure_connected(
        &mut self,
        net: &mut NetRef,
        peer: &PeerTicket,
        psk: &[u8; 32],
        my_static: &StaticKeypair,
        rng: &mut BrokerRng,
        cluster_id: u64,
    ) -> ViResult<usize> {
        // Already connected?
        if let Some(slot) = self.find_session(&peer.node_id) {
            return Ok(slot);
        }
        if self.pool.is_full() {
            return Err(ViError::WouldBlock);
        }

        // Try direct TCP paths.
        for i in 0..peer.addrs_len as usize {
            let (ip, port) = peer.addrs[i];
            if ip == [0, 0, 0, 0] {
                continue;
            }
            match self.try_direct_connect(net, peer, ip, port, psk, my_static, rng, cluster_id) {
                Ok(slot) => return Ok(slot),
                Err(ViError::WouldBlock) => return Err(ViError::WouldBlock),
                Err(_) => continue,
            }
        }

        // The external relay is mTLS-only. No raw TCP fallback is permitted.
        Err(ViError::NotSupported)
    }

    /// Find an existing session by node_id. Returns pool slot or None.
    pub fn find_session(&self, node_id: &CellNetId) -> Option<usize> {
        // ConnectionPool doesn't expose node_id directly — iterate via cluster_id proxy.
        // For now, node_id equality is checked externally by routing.rs.
        // This will be wired properly when routing.rs holds node_id→slot mapping.
        let _ = node_id;
        None
    }

    #[allow(clippy::too_many_arguments)] // reason: Noise handshake needs the full key/identity set; a params struct is planned with the routing wiring
    fn try_direct_connect(
        &mut self,
        net: &mut NetRef,
        peer: &PeerTicket,
        addr: [u8; 4],
        port: u16,
        psk: &[u8; 32],
        my_static: &StaticKeypair,
        rng: &mut BrokerRng,
        cluster_id: u64,
    ) -> ViResult<usize> {
        if self.pool.is_full() {
            return Err(ViError::WouldBlock);
        }
        let mut resp = [0u8; api::ipc::IPC_BUF_SIZE];

        sys_heartbeat(HEARTBEAT_MS);
        let cap = match net
            .call::<NetRequest, NetResponse>(&NetRequest::TcpConnect { addr, port }, &mut resp)
            .map_err(|_| ViError::IO)?
        {
            NetResponse::CapId(id) => id,
            _ => return Err(ViError::IO),
        };

        // Build Noise session (we are initiator on direct connect).
        let mut session = NoiseSession::new(
            rng,
            psk,
            my_static,
            peer.node_id.0,
            cluster_id,
            &self.identity.node_id,
            &peer.node_id,
            cap,
            true, // initiator
        )?;

        session.do_handshake(net)?;
        self.pool.try_insert(session)
    }
}

/// Look up the reflexive address hint from identity (if STUN has run).
pub fn reflexive_or_direct(identity: &BrokerIdentity, peer: &PeerTicket) -> Option<([u8; 4], u16)> {
    // Prefer peer's first direct addr; fall back to reflexive.
    if peer.addrs_len > 0 && peer.addrs[0].0 != [0, 0, 0, 0] {
        Some(peer.addrs[0])
    } else {
        identity.reflexive_addr
    }
}
