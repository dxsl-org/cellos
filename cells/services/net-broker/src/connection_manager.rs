// SPDX-License-Identifier: Apache-2.0
//! ConnectionManager — multi-path peer connection with relay fallback.
//!
//! Connection strategy per peer:
//!   1. Try direct TCP (LAN address from ticket.addrs[0])       — 2s timeout
//!   2. Try direct TCP (STUN reflexive, ticket.addrs[1])        — 2s timeout
//!   3. Fall back: route through relay (ticket.relay_ip:port)
//!
//! The caller (dispatch loop) does not need to know which path is active.
//! All paths end in a Noise KKpsk0 session in the ConnectionPool.

// reason: implements the multi-path (direct TCP / STUN / relay-fallback)
// peer connection manager for the net-broker robot-swarm feature; not yet
// constructed from main.rs — the dispatch loop currently only polls
// `relay_client.is_connected()` directly and has no routing/connection
// wiring (P06/P08 TODOs).
#![allow(dead_code)]

use api::cluster::{CellNetId, PeerTicket};
use api::ipc::{NetRequest, NetResponse};
use ostd::service::NetRef;
use ostd::syscall::sys_heartbeat;
use ostd::{ViError, ViResult};

use crate::identity::BrokerIdentity;
use crate::relay::RelayClient;
use crate::rng::BrokerRng;
use crate::transport::{ConnectionPool, NoiseSession, StaticKeypair};

const HEARTBEAT_MS: u64 = 500;
const CONNECT_TIMEOUT_MS: u32 = 2000;

/// Manages peer connections: direct TCP preferred, relay fallback.
pub struct ConnectionManager<'a> {
    pool: &'a mut ConnectionPool,
    relay: &'a mut RelayClient,
    identity: &'a BrokerIdentity,
}

impl<'a> ConnectionManager<'a> {
    pub fn new(
        pool: &'a mut ConnectionPool,
        relay: &'a mut RelayClient,
        identity: &'a BrokerIdentity,
    ) -> Self {
        Self {
            pool,
            relay,
            identity,
        }
    }

    /// Ensure a Noise session exists for `peer`. Returns the pool slot index.
    ///
    /// Tries direct TCP paths first (up to 2 addrs), then relay.
    /// `psk` is the K1 cluster PSK. `rng` is the broker's PRNG.
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

        // Try direct TCP paths.
        for i in 0..peer.addrs_len as usize {
            let (ip, port) = peer.addrs[i];
            if ip == [0, 0, 0, 0] {
                continue;
            }
            match self.try_direct_connect(net, peer, ip, port, psk, my_static, rng, cluster_id) {
                Ok(slot) => return Ok(slot),
                Err(_) => continue,
            }
        }

        // Relay path: connect relay if not already connected.
        if !self.relay.is_connected() {
            self.relay.connect(net)?;
        }

        // For relay path we don't have a Noise session yet — the relay just forwards
        // raw frames. The Noise handshake happens over the relay channel.
        // Return a sentinel slot that indicates relay-only mode.
        // TODO: implement relay-mediated Noise handshake (requires signaling peer to initiate).
        Err(ViError::NotFound)
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
        let slot = self.pool.insert(session);
        Ok(slot)
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
