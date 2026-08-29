// SPDX-License-Identifier: Apache-2.0
//! BrokerIdentity — per-machine network identity and peer address book.
//!
//! G1 model: CellNetId = X25519 static public key from the Noise keypair.
//! No separate Ed25519 key material is generated; signing comes in G2.
//!
//! Config: /etc/cellos/cluster.cfg, simple key=value format.
//! Export registry: /etc/cellos/c2c-exports.cfg, non-secret key=value format.
//! See doc-comment on `load_config` for the expected layout.

// reason: BrokerIdentity itself is constructed and driven from main.rs, but
// several accessors (peer_count, get_peer_by_node_id, update_reflexive) exist
// for callers that aren't wired yet — connection_manager::reflexive_or_direct
// reads `reflexive_addr` but nothing ever calls `update_reflexive` because
// stun::query_reflexive_addr is itself unwired from the dispatch loop.
#![allow(dead_code)]

extern crate alloc;

use crate::export_registry::{load_remote_exports, RegistrySource, RemoteExports};
use crate::peer_config::{parse_peer_config_bytes, MAX_PEERS};
use crate::relay_config::{parse_relay_endpoint_bytes, RelayEndpoint};
use api::cluster::{CellNetId, PeerTicket};
use ostd::clients::vfs::VfsClient;
use ostd::io::{print, println};

const CFG_PATH: &str = "/etc/cellos/cluster.cfg";
const CFG_READ_MAX_BYTES: usize = 4 * 1024;

#[cfg(test)]
mod tests;

/// Per-machine network identity and peer address book.
pub struct BrokerIdentity {
    pub node_id: CellNetId,
    peers: [Option<PeerTicket>; MAX_PEERS],
    peers_len: usize,
    remote_exports: RemoteExports,
    relay_endpoint: Option<RelayEndpoint>,
    /// Reflexive public address discovered via STUN. Updated by `stun` module.
    pub reflexive_addr: Option<([u8; 4], u16)>,
}

impl BrokerIdentity {
    /// Construct from the X25519 static public key (G1 identity model).
    pub fn from_static_pub(static_pub: [u8; 32]) -> Self {
        Self {
            node_id: CellNetId::from_bytes(static_pub),
            peers: [const { None }; MAX_PEERS],
            peers_len: 0,
            remote_exports: RemoteExports::absent(),
            relay_endpoint: None,
            reflexive_addr: None,
        }
    }

    /// Parse /etc/cellos/cluster.cfg into the peer table.
    ///
    /// Expected format (flat key=value, blank lines and `#` comments ignored):
    /// ```text
    /// peer_count=2
    /// peer_0_node_id=deadbeef...  # 64 hex chars = 32 bytes
    /// peer_0_relay_ip=1.2.3.4
    /// peer_0_relay_port=8765
    /// peer_0_direct=192.168.1.10:4521   # optional
    /// relay_ip=10.0.0.5
    /// relay_port=443
    /// relay_hostname=relay.example
    /// peer_1_node_id=...
    /// peer_1_relay_ip=...
    /// peer_1_relay_port=...
    /// ```
    pub fn load_config(&mut self) {
        let data = match VfsClient::new().read_file_bounded(CFG_PATH, CFG_READ_MAX_BYTES) {
            Ok(d) => d,
            Err(_) => {
                println("[net-broker] failed to read cluster.cfg — no peers configured");
                return;
            }
        };
        self.load_config_bytes(&data);
    }

    fn load_config_bytes(&mut self, data: &[u8]) {
        let (peers, peers_len) = parse_peer_config_bytes(data);
        self.peers = peers;
        self.peers_len = peers_len;
        self.relay_endpoint = match parse_relay_endpoint_bytes(data) {
            Ok(endpoint) => endpoint,
            Err(_) => {
                println("[net-broker] invalid relay endpoint — relay remains disabled");
                None
            }
        };
        print("[net-broker] loaded peers from cluster.cfg: count=");
        ostd::io::print_usize(self.peers_len);
        println("");
    }

    /// Load the non-secret C2C export registry and freeze remote state.
    pub fn load_export_registry(&mut self) {
        let mut source = VfsRegistrySource::new();
        self.remote_exports = load_remote_exports(&mut source);
    }

    pub fn peer_count(&self) -> usize {
        self.peers_len
    }

    pub fn get_peer(&self, idx: usize) -> Option<&PeerTicket> {
        self.peers.get(idx)?.as_ref()
    }

    pub fn get_peer_by_node_id(&self, node_id: &CellNetId) -> Option<&PeerTicket> {
        self.peers[..self.peers_len]
            .iter()
            .find_map(|p| p.as_ref().filter(|t| &t.node_id == node_id))
    }

    pub fn remote_exports(&self) -> &RemoteExports {
        &self.remote_exports
    }

    /// Return the validated relay endpoint without activating relay traffic.
    pub fn relay_endpoint(&self) -> Option<&RelayEndpoint> {
        self.relay_endpoint.as_ref()
    }
    pub fn update_reflexive(&mut self, ip: [u8; 4], port: u16) {
        self.reflexive_addr = Some((ip, port));
    }
}

struct VfsRegistrySource {
    vfs: VfsClient,
}

impl VfsRegistrySource {
    fn new() -> Self {
        Self {
            vfs: VfsClient::new(),
        }
    }
}

impl RegistrySource for VfsRegistrySource {
    fn list_dir(&mut self, path: &str) -> Result<alloc::vec::Vec<u8>, ()> {
        self.vfs.list_dir(path).map_err(|_| ())
    }

    fn stat(&mut self, path: &str) -> Result<(u64, bool), ()> {
        self.vfs.stat(path).map_err(|_| ())
    }

    fn read_file_bounded(
        &mut self,
        path: &str,
        max_bytes: usize,
    ) -> Result<alloc::vec::Vec<u8>, ()> {
        self.vfs.read_file_bounded(path, max_bytes).map_err(|_| ())
    }
}
