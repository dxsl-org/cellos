//! Peer config parser for `/etc/cellos/cluster.cfg`.

mod ascii;

use api::cluster::{CellNetId, PeerTicket};
use ascii::{
    eq_slice, parse_addr, parse_hex32, parse_ipv4, parse_u16_ascii, starts_with, trim_ascii,
};

pub const MAX_PEERS: usize = 8;

#[cfg(test)]
mod ascii_tests;
#[cfg(test)]
mod tests;

pub fn parse_peer_config_bytes(data: &[u8]) -> ([Option<PeerTicket>; MAX_PEERS], usize) {
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

    let mut peers = [const { None }; MAX_PEERS];
    let mut len = 0usize;
    for b in &builders {
        if let Some(ticket) = b.build() {
            if len < MAX_PEERS {
                peers[len] = Some(ticket);
                len += 1;
            }
        }
    }

    (peers, len)
}

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

fn parse_cfg_kv(key: &[u8], val: &[u8], builders: &mut [PeerBuilder; MAX_PEERS]) {
    if !starts_with(key, b"peer_") {
        return;
    }
    let rest = &key[5..];
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
