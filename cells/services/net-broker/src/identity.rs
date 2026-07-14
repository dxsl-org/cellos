// SPDX-License-Identifier: Apache-2.0
//! BrokerIdentity — per-machine network identity and peer address book.
//!
//! G1 model: CellNetId = X25519 static public key from the Noise keypair.
//! No separate Ed25519 key material is generated; signing comes in G2.
//!
//! Config: /etc/cellos/cluster.cfg, simple key=value format.
//! See doc-comment on `load_config` for the expected layout.

// reason: BrokerIdentity itself is constructed and driven from main.rs, but
// several accessors (peer_count, get_peer_by_node_id, update_reflexive) exist
// for callers that aren't wired yet — connection_manager::reflexive_or_direct
// reads `reflexive_addr` but nothing ever calls `update_reflexive` because
// stun::query_reflexive_addr is itself unwired from the dispatch loop.
#![allow(dead_code)]

extern crate alloc;

use api::cluster::{CellNetId, PeerTicket};
use ostd::clients::vfs::VfsClient;
use ostd::io::{print, println};

const MAX_PEERS: usize = 8;
const CFG_PATH: &str = "/etc/cellos/cluster.cfg";

/// Per-machine network identity and peer address book.
pub struct BrokerIdentity {
    pub node_id: CellNetId,
    peers: [Option<PeerTicket>; MAX_PEERS],
    peers_len: usize,
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
    /// peer_1_node_id=...
    /// peer_1_relay_ip=...
    /// peer_1_relay_port=...
    /// ```
    pub fn load_config(&mut self) {
        let data = match VfsClient::new().read_file(CFG_PATH) {
            Ok(d) => d,
            Err(_) => {
                println("[net-broker] /etc/cellos/cluster.cfg not found — no peers configured");
                return;
            }
        };

        let mut builders = [const { PeerBuilder::new() }; MAX_PEERS];

        for line in data.split(|&b| b == b'\n') {
            let line = trim_ascii(line);
            if line.is_empty() || line[0] == b'#' {
                continue;
            }
            let Some(eq) = line.iter().position(|&b| b == b'=') else {
                continue;
            };
            let key = trim_ascii(&line[..eq]);
            let val = trim_ascii(&line[eq + 1..]);
            parse_cfg_kv(key, val, &mut builders);
        }

        for b in &builders {
            if let Some(ticket) = b.build() {
                if self.peers_len < MAX_PEERS {
                    self.peers[self.peers_len] = Some(ticket);
                    self.peers_len += 1;
                }
            }
        }
        print("[net-broker] loaded peers from cluster.cfg: count=");
        ostd::io::print_usize(self.peers_len);
        println("");
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

    pub fn update_reflexive(&mut self, ip: [u8; 4], port: u16) {
        self.reflexive_addr = Some((ip, port));
    }
}

// ── PeerBuilder ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct PeerBuilder {
    valid: bool,
    node_id: Option<[u8; 32]>,
    relay_ip: Option<[u8; 4]>,
    relay_port: Option<u16>,
    direct_ip: Option<[u8; 4]>,
    direct_port: Option<u16>,
}

impl PeerBuilder {
    const fn new() -> Self {
        Self {
            valid: false,
            node_id: None,
            relay_ip: None,
            relay_port: None,
            direct_ip: None,
            direct_port: None,
        }
    }

    fn build(&self) -> Option<PeerTicket> {
        if !self.valid {
            return None;
        }
        let node_id = self.node_id?;
        let relay_ip = self.relay_ip?;
        let relay_port = self.relay_port?;
        let mut addrs = [([0u8; 4], 0u16); 3];
        let mut addrs_len = 0u8;
        if let (Some(ip), Some(port)) = (self.direct_ip, self.direct_port) {
            addrs[0] = (ip, port);
            addrs_len = 1;
        }
        Some(PeerTicket {
            node_id: CellNetId::from_bytes(node_id),
            relay_ip,
            relay_port,
            addrs,
            addrs_len,
        })
    }
}

/// Dispatch a key=value pair into the peer builder array.
/// Key format: `peer_N_field` where N is 0–7.
fn parse_cfg_kv(key: &[u8], val: &[u8], builders: &mut [PeerBuilder; MAX_PEERS]) {
    if !starts_with(key, b"peer_") {
        return;
    }
    let rest = &key[5..];
    // Parse index digit
    if rest.is_empty() || !rest[0].is_ascii_digit() {
        return;
    }
    let idx = (rest[0] - b'0') as usize;
    if idx >= MAX_PEERS {
        return;
    }
    let after_idx = &rest[1..];
    if !starts_with(after_idx, b"_") {
        return;
    }
    let field = &after_idx[1..];

    builders[idx].valid = true;
    if eq_slice(field, b"node_id") {
        builders[idx].node_id = parse_hex32(val);
    } else if eq_slice(field, b"relay_ip") {
        builders[idx].relay_ip = parse_ipv4(val);
    } else if eq_slice(field, b"relay_port") {
        builders[idx].relay_port = parse_u16_ascii(val);
    } else if eq_slice(field, b"direct") {
        if let Some((ip, port)) = parse_addr(val) {
            builders[idx].direct_ip = Some(ip);
            builders[idx].direct_port = Some(port);
        }
    }
}

// ── ASCII helpers (no_std, no alloc) ─────────────────────────────────────────

fn trim_ascii(s: &[u8]) -> &[u8] {
    let s = match s.iter().position(|b| !b.is_ascii_whitespace()) {
        Some(i) => &s[i..],
        None => return &[],
    };
    match s.iter().rposition(|b| !b.is_ascii_whitespace()) {
        Some(i) => &s[..=i],
        None => s,
    }
}

fn starts_with(s: &[u8], prefix: &[u8]) -> bool {
    s.len() >= prefix.len() && &s[..prefix.len()] == prefix
}

fn eq_slice(a: &[u8], b: &[u8]) -> bool {
    a == b
}

fn parse_ipv4(s: &[u8]) -> Option<[u8; 4]> {
    let mut parts = [0u8; 4];
    let mut idx = 0;
    let mut cur: u16 = 0;
    let mut any = false;
    for &b in s {
        if b == b'.' {
            if idx >= 3 {
                return None;
            }
            parts[idx] = cur as u8;
            idx += 1;
            cur = 0;
            any = false;
        } else if b.is_ascii_digit() {
            cur = cur * 10 + (b - b'0') as u16;
            if cur > 255 {
                return None;
            }
            any = true;
        } else {
            return None;
        }
    }
    if !any || idx != 3 {
        return None;
    }
    parts[3] = cur as u8;
    Some(parts)
}

fn parse_u16_ascii(s: &[u8]) -> Option<u16> {
    let mut n: u32 = 0;
    for &b in s {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n * 10 + (b - b'0') as u32;
        if n > 65535 {
            return None;
        }
    }
    Some(n as u16)
}

fn parse_addr(s: &[u8]) -> Option<([u8; 4], u16)> {
    let colon = s.iter().rposition(|&b| b == b':')?;
    let ip = parse_ipv4(&s[..colon])?;
    let port = parse_u16_ascii(&s[colon + 1..])?;
    Some((ip, port))
}

/// Parse 64 hex chars into 32 bytes. Returns None on invalid input.
fn parse_hex32(s: &[u8]) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.chunks_exact(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
