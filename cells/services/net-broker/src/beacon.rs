// reason: this module implements the SwarmBeacon LAN-multicast discovery wire
// protocol (P05) for the net-broker robot-swarm feature. `main.rs` only declares
// `mod beacon;` (main.rs:62) and never references `beacon::` — the dispatch loop's
// beacon-timer and try_recv calls are still TODOs (main.rs:134-135). Not wired yet.
#![allow(dead_code)]

/// SwarmBeacon — XChaCha20-Poly1305 UDP multicast discovery for net-broker.
///
/// Wire frame (80B): nonce[24] || ciphertext[40] || poly1305-tag[16].
/// Plaintext (40B, AEAD-protected):
///   magic[4] version[1] mode[1] pad[2] cluster_id[8] machine_id[8]
///   boot_epoch[8] mono_counter[8]
///
/// Gossip key ≠ K1: XOR domain-separated from K1.
/// Noise does NOT cover gossip — multicast is connectionless, no handshake/session.
extern crate alloc;

use alloc::vec::Vec;
use api::ipc::{NetRequest, NetResponse};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use ostd::service::NetRef;
use ostd::syscall::{sys_get_time, sys_heartbeat};
use rand_core::RngCore;

use crate::rng::BrokerRng;

// ── Constants ─────────────────────────────────────────────────────────────────

const MAGIC: [u8; 4] = *b"VCLS";
const VERSION: u8 = 1;

pub const BEACON_PORT: u16 = 9087;
pub const MULTICAST_GROUP: [u8; 4] = [239, 0, 0, 1];

const PLAIN_LEN: usize = 40;
const NONCE_LEN: usize = 24;
pub const WIRE_LEN: usize = NONCE_LEN + PLAIN_LEN + 16; // 80B

const HEARTBEAT_MS: u64 = 500;

// ── Gossip key derivation ─────────────────────────────────────────────────────

/// Derive gossip AEAD key from K1 (XOR domain separator; gossip key ≠ raw Noise PSK K1).
pub fn derive_gossip_key(k1: &[u8; 32]) -> [u8; 32] {
    const DOM: [u8; 32] = *b"cellos-gossip-xc20p1305-v1-00000";
    let mut k = [0u8; 32];
    for i in 0..32 {
        k[i] = k1[i] ^ DOM[i];
    }
    k
}

// ── BeaconPlain ───────────────────────────────────────────────────────────────

/// 40-byte AEAD-protected beacon plaintext (all fields LE).
#[repr(C)]
pub struct BeaconPlain {
    pub magic: [u8; 4],
    pub version: u8,
    pub mode: u8,
    pub pad: [u8; 2],
    pub cluster_id: u64,
    pub machine_id: u64,
    pub boot_epoch: u64,
    pub mono_counter: u64,
}

const _SIZE_CHECK: () = assert!(core::mem::size_of::<BeaconPlain>() == PLAIN_LEN);

impl BeaconPlain {
    pub fn encode(&self) -> [u8; PLAIN_LEN] {
        let mut b = [0u8; PLAIN_LEN];
        b[..4].copy_from_slice(&self.magic);
        b[4] = self.version;
        b[5] = self.mode;
        b[6..8].copy_from_slice(&self.pad);
        b[8..16].copy_from_slice(&self.cluster_id.to_le_bytes());
        b[16..24].copy_from_slice(&self.machine_id.to_le_bytes());
        b[24..32].copy_from_slice(&self.boot_epoch.to_le_bytes());
        b[32..40].copy_from_slice(&self.mono_counter.to_le_bytes());
        b
    }

    pub fn decode(b: &[u8; PLAIN_LEN]) -> Self {
        Self {
            magic: [b[0], b[1], b[2], b[3]],
            version: b[4],
            mode: b[5],
            pad: [b[6], b[7]],
            cluster_id: u64::from_le_bytes(b[8..16].try_into().unwrap()),
            machine_id: u64::from_le_bytes(b[16..24].try_into().unwrap()),
            boot_epoch: u64::from_le_bytes(b[24..32].try_into().unwrap()),
            mono_counter: u64::from_le_bytes(b[32..40].try_into().unwrap()),
        }
    }
}

// ── Beacon crypto ─────────────────────────────────────────────────────────────

/// Encrypt one beacon → 80B wire frame. Nonce drawn from BrokerRng (fail-closed).
pub fn encrypt_beacon(
    gossip_key: &[u8; 32],
    plain: &BeaconPlain,
    rng: &mut BrokerRng,
) -> [u8; WIRE_LEN] {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill_bytes(&mut nonce_bytes);

    let nonce = XNonce::from_slice(&nonce_bytes);
    let plain_bytes = plain.encode();
    // G1: empty AAD (machine_id + cluster_id are in the encrypted payload).
    // G2 TODO: move to 96B frame with 16B outer unencrypted header for AAD binding.
    let cipher = XChaCha20Poly1305::new(gossip_key.into());
    let ct: Vec<u8> = cipher
        .encrypt(
            nonce,
            Payload {
                msg: &plain_bytes,
                aad: &[],
            },
        )
        .expect("[beacon] encrypt failed");

    let mut wire = [0u8; WIRE_LEN];
    wire[..NONCE_LEN].copy_from_slice(&nonce_bytes);
    wire[NONCE_LEN..].copy_from_slice(&ct);
    wire
}

/// Decrypt and verify one 80B wire frame. Returns None on AEAD failure or bad magic.
pub fn decrypt_beacon(gossip_key: &[u8; 32], wire: &[u8; WIRE_LEN]) -> Option<BeaconPlain> {
    let nonce = XNonce::from_slice(&wire[..NONCE_LEN]);
    let ct = &wire[NONCE_LEN..];
    let cipher = XChaCha20Poly1305::new(gossip_key.into());
    let plain_vec = cipher.decrypt(nonce, Payload { msg: ct, aad: &[] }).ok()?;
    if plain_vec.len() != PLAIN_LEN {
        return None;
    }
    let plain = BeaconPlain::decode(plain_vec[..PLAIN_LEN].try_into().ok()?);
    if plain.magic != MAGIC || plain.version != VERSION {
        return None;
    }
    Some(plain)
}

// ── BeaconChannel ─────────────────────────────────────────────────────────────

/// UDP socket for beacon send/recv.
pub struct BeaconChannel {
    cap_id: u32,
}

impl BeaconChannel {
    /// Create, bind, and join multicast. Call at Init; first RECV goes in dispatch loop.
    pub fn init(net: &mut NetRef) -> Option<Self> {
        let mut resp = [0u8; api::ipc::IPC_BUF_SIZE];
        let cap_id = match net
            .call::<NetRequest, NetResponse>(&NetRequest::UdpCreate, &mut resp)
            .ok()?
        {
            NetResponse::CapId(id) => id,
            _ => return None,
        };
        let _ = net.call::<NetRequest, NetResponse>(
            &NetRequest::UdpBind {
                cap_id,
                port: BEACON_PORT,
            },
            &mut resp,
        );
        let _ = net.call::<NetRequest, NetResponse>(
            &NetRequest::MulticastJoin {
                cap_id,
                group: MULTICAST_GROUP,
            },
            &mut resp,
        );
        Some(Self { cap_id })
    }

    pub fn send_frame(&self, net: &mut NetRef, frame: &[u8; WIRE_LEN]) {
        let mut resp = [0u8; api::ipc::IPC_BUF_SIZE];
        sys_heartbeat(HEARTBEAT_MS);
        let _ = net.call::<NetRequest, NetResponse>(
            &NetRequest::UdpSend {
                cap_id: self.cap_id,
                addr: MULTICAST_GROUP,
                port: BEACON_PORT,
                data: frame,
            },
            &mut resp,
        );
    }

    /// Non-blocking: returns None if no frame is available.
    pub fn try_recv_frame(&self, net: &mut NetRef) -> Option<[u8; WIRE_LEN]> {
        let mut resp = [0u8; api::ipc::IPC_BUF_SIZE];
        match net
            .call::<NetRequest, NetResponse>(
                &NetRequest::UdpRecv {
                    cap_id: self.cap_id,
                    buf_len: WIRE_LEN as u32,
                },
                &mut resp,
            )
            .ok()?
        {
            NetResponse::Data(d) if d.len() == WIRE_LEN => d.try_into().ok(),
            _ => None,
        }
    }
}

// ── PeerTable ─────────────────────────────────────────────────────────────────

pub struct PeerEntry {
    pub machine_id: u64,
    /// Peer's X25519 static pub key; populated once learned (may be in beacon payload
    /// once we extend the plaintext in G2, or provided by a separate key-exchange).
    pub static_pub: Option<[u8; 32]>,
    pub last_epoch: u64,
    pub last_counter: u64,
    pub last_heard_mono: u64,
}

pub struct PeerTable {
    entries: [Option<PeerEntry>; 8],
}

impl PeerTable {
    pub const fn new() -> Self {
        Self {
            entries: [const { None }; 8],
        }
    }

    /// Update from a verified beacon. Returns true if this is a NEW peer.
    pub fn update(&mut self, plain: &BeaconPlain) -> bool {
        let now = sys_get_time();
        for e in self.entries.iter_mut().flatten() {
            if e.machine_id == plain.machine_id {
                if plain.boot_epoch == e.last_epoch && plain.mono_counter <= e.last_counter {
                    return false; // anti-replay: reject non-increasing counter
                }
                e.last_epoch = plain.boot_epoch;
                e.last_counter = plain.mono_counter;
                e.last_heard_mono = now;
                return false;
            }
        }
        // New peer — insert in first empty slot.
        for slot in self.entries.iter_mut() {
            if slot.is_none() {
                *slot = Some(PeerEntry {
                    machine_id: plain.machine_id,
                    static_pub: None,
                    last_epoch: plain.boot_epoch,
                    last_counter: plain.mono_counter,
                    last_heard_mono: now,
                });
                return true;
            }
        }
        false // table full
    }

    pub fn timed_out_count(&self, timeout_ms: u64) -> usize {
        let now = sys_get_time();
        self.entries
            .iter()
            .flatten()
            .filter(|e| now.wrapping_sub(e.last_heard_mono) > timeout_ms)
            .count()
    }
}
