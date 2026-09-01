// SwarmBeacon LAN-multicast discovery for net-broker.
// The runtime creates its channel only after K1-derived gossip setup succeeds;
// receive polling runs in the broker's dedicated network role.
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
use api::cluster::CellNetId;
use api::ipc::{NetRequest, NetResponse};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use ostd::service::NetRef;
use ostd::syscall::{sys_get_time_ms, sys_heartbeat};
use rand_core::RngCore;
use sha2::{Digest, Sha256};

use crate::rng::BrokerRng;

const MAGIC: [u8; 4] = *b"VCLS";
const VERSION: u8 = 1;

pub const BEACON_PORT: u16 = 9087;
pub const MULTICAST_GROUP: [u8; 4] = [239, 0, 0, 1];

const PLAIN_LEN: usize = 40;
const NONCE_LEN: usize = 24;
pub const WIRE_LEN: usize = NONCE_LEN + PLAIN_LEN + 16; // 80B

const UDP_SOURCE_HEADER_LEN: usize = 6;
const UDP_RESPONSE_LEN: usize = UDP_SOURCE_HEADER_LEN + WIRE_LEN;

const HEARTBEAT_MS: u64 = 500;
/// Spec 14 `BEACON_INTERVAL_MS`; broker timer deadlines are monotonic.
pub const BEACON_INTERVAL_MS: u64 = 1_000;
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

const MACHINE_ID_DOMAIN: &[u8] = b"cellos-machine-id-v1";

/// Derive the only accepted wire machine ID for a configured Noise identity.
pub fn derive_machine_id(node_id: &CellNetId) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(MACHINE_ID_DOMAIN);
    hasher.update(node_id.as_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes(digest[..8].try_into().expect("SHA-256 digest length"))
}

pub const fn beacon_due(now_ms: u64, deadline_ms: u64) -> bool {
    now_ms >= deadline_ms
}

pub const fn next_beacon_deadline(now_ms: u64) -> u64 {
    now_ms.saturating_add(BEACON_INTERVAL_MS)
}

/// Reject authenticated frames that are not routable to this configured broker.
/// The caller supplies the configured-peer lookup because beacons intentionally
/// carry no unauthenticated static public key.
pub fn accepts_peer_beacon<F>(
    plain: &BeaconPlain,
    cluster_id: u64,
    local_machine_id: u64,
    mut is_configured_machine_id: F,
) -> bool
where
    F: FnMut(u64) -> bool,
{
    plain.cluster_id == cluster_id
        && plain.machine_id != local_machine_id
        && is_configured_machine_id(plain.machine_id)
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
    pub const fn local(
        cluster_id: u64,
        machine_id: u64,
        boot_epoch: u64,
        mono_counter: u64,
    ) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            mode: 0,
            pad: [0; 2],
            cluster_id,
            machine_id,
            boot_epoch,
            mono_counter,
        }
    }

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

const BEACON_IPC_TIMEOUT_TICKS: u64 = service_net_broker::runtime_roles::NETWORK_IPC_TIMEOUT_TICKS;

impl BeaconChannel {
    fn call_after_admission<'r>(
        service_tid: usize,
        request: &NetRequest<'_>,
        send_buffer: &mut [u8; api::ipc::IPC_BUF_SIZE],
        response_buffer: &'r mut [u8; api::ipc::IPC_BUF_SIZE],
    ) -> Result<NetResponse<'r>, ()> {
        let encoded = api::ipc::encode(request, send_buffer).map_err(|_| ())?;
        if !matches!(
            ostd::syscall::sys_send(service_tid, encoded),
            ostd::syscall::SyscallResult::Ok(_)
        ) {
            return Err(());
        }
        match ostd::syscall::sys_recv_timeout(
            service_tid,
            response_buffer,
            BEACON_IPC_TIMEOUT_TICKS,
        ) {
            ostd::syscall::SyscallResult::Ok(sender) if sender == service_tid => {
                api::ipc::decode(response_buffer).map_err(|_| ())
            }
            _ => Err(()),
        }
    }
    pub fn init(net: &mut NetRef) -> Option<Self> {
        let mut resp = [0u8; api::ipc::IPC_BUF_SIZE];
        let cap_id = match net
            .call::<NetRequest, NetResponse>(&NetRequest::UdpCreate, &mut resp)
            .ok()?
        {
            NetResponse::CapId(id) => id,
            _ => return None,
        };
        let bind = net
            .call::<NetRequest, NetResponse>(
                &NetRequest::UdpBind {
                    cap_id,
                    port: BEACON_PORT,
                },
                &mut resp,
            )
            .ok()?;
        if !response_is_ok(bind) {
            return None;
        }
        let join = net
            .call::<NetRequest, NetResponse>(
                &NetRequest::MulticastJoin {
                    cap_id,
                    group: MULTICAST_GROUP,
                },
                &mut resp,
            )
            .ok()?;
        if !response_is_ok(join) {
            return None;
        }
        Some(Self { cap_id })
    }

    pub fn send_frame(&self, net: &mut NetRef, frame: &[u8; WIRE_LEN]) -> Result<bool, ()> {
        let service_tid = net.resolve().ok_or(())?;
        let mut request = [0u8; api::ipc::IPC_BUF_SIZE];
        let mut response = [0u8; api::ipc::IPC_BUF_SIZE];
        sys_heartbeat(HEARTBEAT_MS);
        let result = Self::call_after_admission(
            service_tid,
            &NetRequest::UdpSend {
                cap_id: self.cap_id,
                addr: MULTICAST_GROUP,
                port: BEACON_PORT,
                data: frame,
            },
            &mut request,
            &mut response,
        );
        match result {
            Ok(response) => Ok(response_sent_full_frame(response)),
            Err(_) => {
                net.invalidate();
                Err(())
            }
        }
    }

    pub fn try_recv_frame(&self, net: &mut NetRef) -> Result<Option<[u8; WIRE_LEN]>, ()> {
        #[cfg(feature = "restart-oracle")]
        if crate::local_runtime::restart_oracle::shutdown_requested() {
            return Ok(None);
        }
        let service_tid = net.resolve().ok_or(())?;
        let mut request = [0u8; api::ipc::IPC_BUF_SIZE];
        let mut response = [0u8; api::ipc::IPC_BUF_SIZE];
        let result = Self::call_after_admission(
            service_tid,
            &NetRequest::UdpRecv {
                cap_id: self.cap_id,
                // The net cell adds the source envelope to its response, not this
                // receive capacity. Preserve the exact bounded wire-frame read.
                buf_len: WIRE_LEN as u32,
            },
            &mut request,
            &mut response,
        );
        match result {
            Ok(NetResponse::Data(data)) => Ok(decode_udp_frame(data)),
            Ok(_) => Ok(None),
            Err(_) => {
                net.invalidate();
                Err(())
            }
        }
    }
}
fn response_is_ok(response: NetResponse<'_>) -> bool {
    matches!(response, NetResponse::Ok)
}

fn response_sent_full_frame(response: NetResponse<'_>) -> bool {
    matches!(
        response,
        NetResponse::Data(data)
            if data == (WIRE_LEN as u32).to_le_bytes().as_slice()
    )
}

fn decode_udp_frame(data: &[u8]) -> Option<[u8; WIRE_LEN]> {
    if data.len() != UDP_RESPONSE_LEN {
        return None;
    }
    data[UDP_SOURCE_HEADER_LEN..].try_into().ok()
}

/// A reboot is authenticated by the beacon AEAD but starts its monotonic
/// counter over. Only a strictly higher boot epoch may reset that counter.
fn is_fresh_beacon(boot_epoch: u64, mono_counter: u64, last_epoch: u64, last_counter: u64) -> bool {
    boot_epoch > last_epoch || (boot_epoch == last_epoch && mono_counter > last_counter)
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
        let Some(now) = sys_get_time_ms() else {
            return false;
        };
        self.update_at(plain, now)
    }

    fn update_at(&mut self, plain: &BeaconPlain, now: u64) -> bool {
        for e in self.entries.iter_mut().flatten() {
            if e.machine_id == plain.machine_id {
                if !is_fresh_beacon(
                    plain.boot_epoch,
                    plain.mono_counter,
                    e.last_epoch,
                    e.last_counter,
                ) {
                    return false;
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
        let Some(now) = sys_get_time_ms() else {
            return 0;
        };
        self.entries
            .iter()
            .flatten()
            .filter(|e| now.wrapping_sub(e.last_heard_mono) > timeout_ms)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_id_uses_the_specified_sha256_domain_separation() {
        let node_id = CellNetId::from_bytes([0; 32]);
        assert_eq!(derive_machine_id(&node_id), 0x956c_1784_7227_82e2);
    }

    #[test]
    fn local_beacon_has_specified_identity_and_monotonic_schedule() {
        let plain = BeaconPlain::local(7, 11, 13, 17);
        assert_eq!(plain.magic, MAGIC);
        assert_eq!(plain.version, VERSION);
        assert_eq!(plain.cluster_id, 7);
        assert_eq!(plain.machine_id, 11);
        assert_eq!(plain.boot_epoch, 13);
        assert_eq!(plain.mono_counter, 17);
        assert!(beacon_due(1_000, 1_000));
        assert!(!beacon_due(999, 1_000));
        assert_eq!(next_beacon_deadline(1_000), 2_000);
        assert_eq!(next_beacon_deadline(u64::MAX), u64::MAX);
    }

    #[test]
    fn peer_beacon_requires_cluster_and_configured_machine_identity() {
        let plain = BeaconPlain::local(7, 11, 1, 0);
        assert!(accepts_peer_beacon(&plain, 7, 12, |machine_id| machine_id == 11));
        assert!(!accepts_peer_beacon(&plain, 8, 12, |_| true));
        assert!(!accepts_peer_beacon(&plain, 7, 11, |_| true));
        assert!(!accepts_peer_beacon(&plain, 7, 12, |_| false));
    }
    #[test]
    fn prefixed_valid_datagram_decrypts_after_envelope_removal() {
        let key = derive_gossip_key(&[0x11; 32]);
        let plain = BeaconPlain {
            magic: MAGIC,
            version: VERSION,
            mode: 0,
            pad: [0; 2],
            cluster_id: 42,
            machine_id: 7,
            boot_epoch: 3,
            mono_counter: 1,
        };
        let nonce = [0x22; NONCE_LEN];
        let cipher = XChaCha20Poly1305::new_from_slice(&key).unwrap();
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plain.encode(),
                    aad: b"",
                },
            )
            .unwrap();
        let mut response = [0u8; UDP_RESPONSE_LEN];
        response[..UDP_SOURCE_HEADER_LEN].copy_from_slice(&[192, 0, 2, 1, 0x7f, 0x23]);
        response[UDP_SOURCE_HEADER_LEN..UDP_SOURCE_HEADER_LEN + NONCE_LEN].copy_from_slice(&nonce);
        response[UDP_SOURCE_HEADER_LEN + NONCE_LEN..].copy_from_slice(&ciphertext);

        let frame = decode_udp_frame(&response).expect("bounded frame");
        let decrypted = decrypt_beacon(&key, &frame).expect("authenticated beacon");
        assert_eq!(decrypted.machine_id, plain.machine_id);
        assert_eq!(decrypted.boot_epoch, plain.boot_epoch);
        assert_eq!(decrypted.mono_counter, plain.mono_counter);
    }

    #[test]
    fn udp_envelope_extracts_only_the_bounded_wire_frame() {
        let mut response = [0u8; UDP_RESPONSE_LEN];
        response[..UDP_SOURCE_HEADER_LEN].copy_from_slice(&[192, 0, 2, 1, 0x7f, 0x23]);
        response[UDP_SOURCE_HEADER_LEN..].fill(0xa5);

        assert_eq!(decode_udp_frame(&response), Some([0xa5; WIRE_LEN]));
        assert_eq!(decode_udp_frame(&response[..UDP_RESPONSE_LEN - 1]), None);
        assert_eq!(decode_udp_frame(&[0; UDP_RESPONSE_LEN + 1]), None);
    }

    #[test]
    fn channel_setup_and_send_require_explicit_success_responses() {
        assert!(response_is_ok(NetResponse::Ok));
        assert!(!response_is_ok(NetResponse::Err(0xff)));
        assert!(!response_is_ok(NetResponse::CapId(1)));

        let sent = (WIRE_LEN as u32).to_le_bytes();
        assert!(response_sent_full_frame(NetResponse::Data(&sent)));
        assert!(!response_sent_full_frame(NetResponse::Data(&[0; 4])));
        assert!(!response_sent_full_frame(NetResponse::Ok));
    }

    #[test]
    fn higher_boot_epoch_rebaselines_the_replay_counter() {
        let mut peers = PeerTable::new();
        let mut plain = BeaconPlain {
            magic: MAGIC,
            version: VERSION,
            mode: 0,
            pad: [0; 2],
            cluster_id: 42,
            machine_id: 7,
            boot_epoch: 7,
            mono_counter: u64::MAX,
        };
        assert!(peers.update_at(&plain, 10));

        plain.boot_epoch = 8;
        plain.mono_counter = 0;
        assert!(!peers.update_at(&plain, 11));
        let peer = peers.entries[0].as_ref().expect("known peer");
        assert_eq!(peer.last_epoch, 8);
        assert_eq!(peer.last_counter, 0);

        plain.boot_epoch = 7;
        plain.mono_counter = u64::MAX;
        assert!(!peers.update_at(&plain, 12));
        let peer = peers.entries[0].as_ref().expect("known peer");
        assert_eq!(peer.last_epoch, 8);
        assert_eq!(peer.last_counter, 0);
    }
}
