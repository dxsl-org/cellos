// Dead-code allowed: these types are the P04 API surface; they are wired into
// the dispatch loop in P06 (RemoteServiceProxy) and P07 (testbed integration).
#![allow(dead_code)]

/// Noise KKpsk0 p2p transport layer for net-broker.
///
/// Provides:
/// - `ClusterKeySource` + `VfsFileKeySource` — load K1 PSK from VFS.
/// - `StaticKeypair` — per-broker X25519 static key (generated at Init).
/// - `NoiseSession` — drives the KKpsk0 handshake over a TCP cap_id, then
///   provides encrypted transport-record send/recv.
/// - `ConnectionPool` — bounded K=4 sessions with fail-closed admission.
///
/// ## Design invariants
/// - K1 held only in broker RAM; never logged, never transmitted plaintext.
/// - cluster_id is bound as Noise prologue — routing mismatch logged separately
///   from PSK failure (invariant from plan §cross-cutting #11).
/// - TCP framing: 2-byte LE length prefix per Noise message/record.
/// - Pool cap K=4 ≤ net cell's 18-socket budget (DHCP/ARP/users compete).
extern crate alloc;

use alloc::vec::Vec;
use clatter::{traits::Dh, KeyPair};
use ostd::{clients::vfs::VfsClient, ViError, ViResult};

use crate::rng::BrokerRng;
use service_net_broker::c2c_envelope::NOISE_TAG_LEN;
use service_net_broker::kms_dh::{KmsBackedX25519, OpaqueStaticKey};

mod connection_pool;
mod noise_session;
mod tcp_framing;

pub use connection_pool::ConnectionPool;
pub use noise_session::NoiseSession;
use tcp_framing::{read_message as tcp_read_msg, write_message as tcp_write_msg};

#[cfg(test)]
mod tests;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Max concurrent Noise sessions.
const MAX_SESSIONS: usize = 4;
const K1_MIN_BYTES: usize = 32;
const K1_READ_MAX_BYTES: usize = 64;

/// Noise handshake message buffer (KKpsk0 max msg ≈ 96 B; room to spare).
const NOISE_MSG_BUF: usize = 256;

/// Noise AEAD tag overhead per transport record.
const NOISE_TAG: usize = NOISE_TAG_LEN;

/// Heartbeat re-arm during handshake (must stay < watchdog interval).
const HEARTBEAT_MS: u64 = 500;

// ── ClusterKeySource ──────────────────────────────────────────────────────────

/// Load the 32-byte cluster PSK (K1). Trait so K2/K3 variants need no call-site changes.
pub trait ClusterKeySource {
    fn load(&self) -> ViResult<[u8; 32]>;
}

/// Load K1 from a VFS path (e.g. `/etc/cellos/cluster.key`).
/// File must contain ≥ 32 bytes; only the first 32 are used as K1.
pub struct VfsFileKeySource {
    pub path: &'static str,
}

impl ClusterKeySource for VfsFileKeySource {
    fn load(&self) -> ViResult<[u8; 32]> {
        load_vfs_key(self.path, |path, max_bytes| {
            VfsClient::new().read_file_bounded(path, max_bytes)
        })
    }
}

fn load_vfs_key<F>(path: &str, read_file: F) -> ViResult<[u8; 32]>
where
    F: FnOnce(&str, usize) -> ViResult<Vec<u8>>,
{
    let data = read_file(path, K1_READ_MAX_BYTES)?;
    if data.len() < K1_MIN_BYTES {
        return Err(ViError::IO);
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&data[..K1_MIN_BYTES]);
    Ok(key)
}

/// X25519 static keypair for either local-only ephemeral or opaque KMS identity.
pub struct StaticKeypair {
    inner: KeyPair<<KmsBackedX25519 as Dh>::PubKey, <KmsBackedX25519 as Dh>::PrivateKey>,
}

impl StaticKeypair {
    /// Generate an ephemeral static key for the remote-disabled local path.
    pub fn generate(rng: &mut BrokerRng) -> Self {
        Self {
            inner: KmsBackedX25519::genkey_rng(rng).expect("[net-broker] static keygen failed"),
        }
    }

    /// Construct from KMS metadata without importing private key bytes.
    pub fn from_opaque(key: OpaqueStaticKey) -> Self {
        Self {
            inner: key.into_keypair(),
        }
    }

    /// Public key bytes used as the G1 `CellNetId`.
    pub fn public_bytes(&self) -> [u8; 32] {
        self.inner.public
    }
}
