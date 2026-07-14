// SPDX-License-Identifier: Apache-2.0
//! Cluster-membership types and the [`declare_cluster!`] macro.
//!
//! A Cell announces cluster participation by embedding a `__ViCell_cluster`
//! ELF section (emitted by [`declare_cluster!`]).  The kernel loader reads this
//! section at spawn time and writes the parsed mode and cluster ID into the TCB.
//!
//! ## ClusterId is NOT a credential
//!
//! [`ClusterId`] is a **routing / dedup identifier** (FNV-1a-64 of the cluster
//! name).  It MUST NOT be used for any access decision.  The net-broker's Noise
//! KKpsk0 handshake (PSK possession) is the sole authentication mechanism.
//! A `ClusterId` mismatch is a routing reject logged *distinctly* from an auth
//! failure — never collapse the two, and a routing reject must not leak whether
//! the PSK would have matched.
//!
//! ## RT cells are barred from cluster participation
//!
//! A cell spawned with `TaskPriority::RealTime` AND a non-`Isolated`
//! `__ViCell_cluster` section is rejected at `SpawnPinned` time (the only
//! place both the priority argument and the loaded `cluster_mode` are
//! simultaneously available).  RT cells must not enter the cluster.

/// Cluster participation mode for a Cell.
///
/// Encoded as a single `u8` at offset 0 of the [`ClusterSection`].
/// `Isolated` (0) is the default for any Cell that omits the section,
/// preserving full backwards compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ClusterMode {
    /// Not a cluster participant (default). Full SAS isolation.
    Isolated = 0,
    /// Publicly reachable by any cluster peer.
    Public = 1,
    /// Private to the named cluster — reachable only by peers sharing the same K1 PSK.
    Private = 2,
}

/// Cluster routing identifier — FNV-1a-64 hash of the cluster name string.
///
/// ⚠️ **Routing only, NOT a credential.**  A `ClusterId` mismatch is a routing
/// reject (logged before the Noise handshake) and MUST be logged distinctly from
/// an authentication failure.  Possession of the correct K1 PSK (proved by
/// completing the Noise KKpsk0 handshake) is the sole authentication mechanism.
/// Two different names may theoretically produce the same hash — that costs a
/// rejected handshake at worst; it is not a security boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ClusterId(pub u64);

impl ClusterId {
    /// Compute the [`ClusterId`] for `name` at **const-evaluation** time.
    ///
    /// Algorithm: FNV-1a-64 (offset basis = 14695981039346656037, prime = 1099511628211).
    /// Stable and deterministic across architectures.
    pub const fn from_name(name: &str) -> ClusterId {
        const FNV_OFFSET: u64 = 14_695_981_039_346_656_037;
        const FNV_PRIME: u64 = 1_099_511_628_211;
        let bytes = name.as_bytes();
        let mut hash = FNV_OFFSET;
        let mut i = 0;
        while i < bytes.len() {
            hash ^= bytes[i] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            i += 1;
        }
        ClusterId(hash)
    }

    /// The zero cluster id — corresponds to `ClusterMode::Isolated`.
    pub const NONE: ClusterId = ClusterId(0);
}

/// Fixed 16-byte `#[repr(C)]` layout stored in the `__ViCell_cluster` ELF section.
///
/// Written by [`declare_cluster!`] at compile time; read by the kernel loader
/// at spawn time.  Layout is little-endian on all supported architectures.
///
/// ```text
/// offset  0 :  u8   mode       (0=Isolated  1=Public  2=Private)
/// offset  1 :  u8   _pad[7]    (zero-fill)
/// offset  8 :  u64  cluster_id (LE, FNV-1a-64 of the cluster name)
/// total: 16 bytes
/// ```
///
/// The loader tolerates a section larger than 16 bytes (forward-compat) and
/// ignores trailing bytes.
#[repr(C)]
pub struct ClusterSection {
    pub mode: u8,
    pub _pad: [u8; 7],
    /// Little-endian cluster routing id.  See [`ClusterId`] docs on routing vs. auth.
    pub cluster_id: u64,
}

impl ClusterSection {
    /// Build a `ClusterSection` for `mode` and cluster `name` at **const-eval** time.
    pub const fn new(mode: ClusterMode, name: &str) -> ClusterSection {
        let id = ClusterId::from_name(name);
        ClusterSection {
            mode: mode as u8,
            _pad: [0u8; 7],
            cluster_id: id.0,
        }
    }
}

// ── CellNetId ─────────────────────────────────────────────────────────────────

/// Stable per-machine network identity — 32-byte public key.
///
/// G1: derived from the X25519 Noise static keypair (no separate Ed25519 keygen).
/// G2: proper Ed25519 public key with signing capability.
///
/// Bound into the Noise prologue so both machines must agree on each other's
/// NodeId during the handshake — prevents routing spoofing (FATAL-1 fix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct CellNetId(pub [u8; 32]);

impl CellNetId {
    pub const ZERO: Self = Self([0u8; 32]);
    pub const fn from_bytes(b: [u8; 32]) -> Self {
        Self(b)
    }
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// ── PeerTicket ────────────────────────────────────────────────────────────────

/// Peer address book — how to connect to one remote Cellos machine.
///
/// Fixed-size, IPv4-only. No hostname fields (DNS resolver absent in G1).
/// Max encoded size: 57 bytes (fits comfortably in IPC_BUF_SIZE).
#[derive(Debug, Clone, Copy)]
pub struct PeerTicket {
    pub node_id: CellNetId,
    pub relay_ip: [u8; 4],
    pub relay_port: u16,
    /// Direct IPv4:port addrs (LAN address + STUN reflexive, up to 3).
    pub addrs: [([u8; 4], u16); 3],
    pub addrs_len: u8,
}

impl PeerTicket {
    pub const ENCODED_MAX: usize = 32 + 4 + 2 + 1 + 3 * 6; // = 57

    pub fn encode(&self, out: &mut [u8]) -> usize {
        assert!(out.len() >= Self::ENCODED_MAX);
        let mut p = 0;
        out[p..p + 32].copy_from_slice(&self.node_id.0);
        p += 32;
        out[p..p + 4].copy_from_slice(&self.relay_ip);
        p += 4;
        out[p..p + 2].copy_from_slice(&self.relay_port.to_le_bytes());
        p += 2;
        out[p] = self.addrs_len;
        p += 1;
        for i in 0..self.addrs_len as usize {
            out[p..p + 4].copy_from_slice(&self.addrs[i].0);
            p += 4;
            out[p..p + 2].copy_from_slice(&self.addrs[i].1.to_le_bytes());
            p += 2;
        }
        p
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 39 {
            return None;
        }
        let mut node_id = [0u8; 32];
        node_id.copy_from_slice(&data[0..32]);
        let mut relay_ip = [0u8; 4];
        relay_ip.copy_from_slice(&data[32..36]);
        let relay_port = u16::from_le_bytes([data[36], data[37]]);
        let addrs_len = (data[38] as usize).min(3) as u8;
        let mut addrs = [([0u8; 4], 0u16); 3];
        let mut p = 39;
        for addr in addrs.iter_mut().take(addrs_len as usize) {
            if p + 6 > data.len() {
                break;
            }
            let mut ip = [0u8; 4];
            ip.copy_from_slice(&data[p..p + 4]);
            let port = u16::from_le_bytes([data[p + 4], data[p + 5]]);
            *addr = (ip, port);
            p += 6;
        }
        Some(Self {
            node_id: CellNetId(node_id),
            relay_ip,
            relay_port,
            addrs,
            addrs_len,
        })
    }
}

/// Emit the `__ViCell_cluster` ELF section declaring this Cell's cluster mode.
///
/// The section is read by the kernel loader at spawn time.  Cells that omit
/// this macro default to `ClusterMode::Isolated` and are unaffected by cluster
/// operations.
///
/// # Example
///
/// ```ignore
/// api::declare_cluster!(mode = Private, name = "robots");
/// ```
///
/// `mode` must be one of `Isolated`, `Public`, or `Private`.
/// `name` is the cluster name string used to derive the [`ClusterId`].
#[macro_export]
macro_rules! declare_cluster {
    (mode = $mode:ident, name = $name:literal) => {
        #[used]
        #[link_section = "__ViCell_cluster"]
        pub static VICELL_CLUSTER: $crate::cluster::ClusterSection =
            $crate::cluster::ClusterSection::new($crate::cluster::ClusterMode::$mode, $name);
    };
}
