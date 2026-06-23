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
/// - `ConnectionPool` — bounded K=4 sessions with stamp-based LRU eviction.
///
/// ## Design invariants
/// - K1 held only in broker RAM; never logged, never transmitted plaintext.
/// - cluster_id is bound as Noise prologue — routing mismatch logged separately
///   from PSK failure (invariant from plan §cross-cutting #11).
/// - TCP framing: 2-byte LE length prefix per Noise message/record.
/// - Pool cap K=4 ≤ net cell's 18-socket budget (DHCP/ARP/users compete).
extern crate alloc;

use alloc::boxed::Box;
use clatter::{
    crypto::{cipher::ChaChaPoly, dh::X25519, hash::Sha256},
    handshakepattern::noise_kk_psk0,
    traits::{Dh, Handshaker},
    transportstate::TransportState,
    KeyPair, NqHandshakeCore,
};
use ostd::{clients::vfs::VfsClient, syscall::sys_heartbeat, ViError, ViResult};
use ostd::service::NetRef;
use api::ipc::{NetRequest, NetResponse};

use crate::rng::BrokerRng;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Max concurrent Noise sessions.
const MAX_SESSIONS: usize = 4;

/// Noise handshake message buffer (KKpsk0 max msg ≈ 96 B; room to spare).
const NOISE_MSG_BUF: usize = 256;

/// Noise AEAD tag overhead per transport record.
const NOISE_TAG: usize = 16;

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
        let data = VfsClient::new().read_file(self.path)?;
        if data.len() < 32 {
            return Err(ViError::IO);
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&data[..32]);
        Ok(key)
    }
}

// ── StaticKeypair ─────────────────────────────────────────────────────────────

/// X25519 static keypair generated at broker Init via BrokerRng.
/// Held in broker RAM only; public half broadcast via P05 beacon.
pub struct StaticKeypair {
    inner: KeyPair<<X25519 as Dh>::PubKey, <X25519 as Dh>::PrivateKey>,
}

impl StaticKeypair {
    pub fn generate(rng: &mut BrokerRng) -> Self {
        Self { inner: X25519::genkey_rng(rng).expect("[net-broker] static keygen failed") }
    }

    /// Public key bytes — share with cluster peers via P05 beacon.
    pub fn public_bytes(&self) -> [u8; 32] {
        self.inner.public
    }
}

// ── NoiseSession ──────────────────────────────────────────────────────────────

type Hs = NqHandshakeCore<X25519, ChaChaPoly, Sha256, BrokerRng>;
type Ts = TransportState<ChaChaPoly, Sha256>;

enum Phase {
    Handshake(Box<Hs>),
    Transport(Box<Ts>),
    /// Sentinel used only during the finalize() call to move out of the Box.
    Finalizing,
}

/// A single KKpsk0 session over a net-cell TCP socket (cap_id).
pub struct NoiseSession {
    phase: Phase,
    pub cap_id: u32,
    /// Which cluster this session belongs to (routing guard).
    pub cluster_id: u64,
}

impl NoiseSession {
    /// Construct and begin a new session; handshake is driven by `do_handshake()`.
    ///
    /// Caller must have already verified `cluster_id` matches the local broker's
    /// cluster before calling this — a mismatch here means the TCP connection was
    /// accepted from the wrong cluster.
    pub fn new(
        rng: &mut BrokerRng,
        psk: &[u8; 32],
        my_static: &StaticKeypair,
        peer_static_pub: [u8; 32],
        cluster_id: u64,
        cap_id: u32,
        is_initiator: bool,
    ) -> ViResult<Self> {
        // Pre-generate our ephemeral key so clatter never calls BrokerRng::default().
        let ephemeral = X25519::genkey_rng(rng).map_err(|_| ViError::IO)?;
        let prologue = cluster_id.to_le_bytes();

        let mut hs = NqHandshakeCore::<X25519, ChaChaPoly, Sha256, BrokerRng>::new(
            noise_kk_psk0(),
            &prologue,
            is_initiator,
            Some(KeyPair {
                public: my_static.inner.public,
                secret: my_static.inner.secret.clone(),
            }),
            Some(ephemeral),
            Some(peer_static_pub),
            None,
        ).map_err(|_| ViError::InvalidArgument)?;

        hs.push_psk(psk);

        Ok(Self {
            phase: Phase::Handshake(Box::new(hs)),
            cap_id,
            cluster_id,
        })
    }

    /// Drive the KKpsk0 handshake to completion.
    ///
    /// KKpsk0 is 2-message: initiator sends msg1, responder sends msg2.
    /// Re-arms the heartbeat at every network boundary.
    pub fn do_handshake(&mut self, net: &mut NetRef) -> ViResult<()> {
        let mut buf = [0u8; NOISE_MSG_BUF];
        let cap_id = self.cap_id; // copy before any &mut self borrow
        let is_init = match &self.phase {
            Phase::Handshake(hs) => hs.is_initiator(),
            _ => return Ok(()),
        };

        if is_init {
            // Initiator: write msg1 → send → wait msg2 → read.
            sys_heartbeat(HEARTBEAT_MS);
            let n = self.handshake_mut()?.write_message(&[], &mut buf).map_err(|_| ViError::IO)?;
            tcp_write_msg(net, cap_id, &buf[..n])?;

            sys_heartbeat(HEARTBEAT_MS);
            let n = tcp_read_msg(net, cap_id, &mut buf)?;
            self.handshake_mut()?.read_message(&buf[..n], &mut []).map_err(|_| ViError::IO)?;
        } else {
            // Responder: wait msg1 → read → write msg2 → send.
            sys_heartbeat(HEARTBEAT_MS);
            let n = tcp_read_msg(net, cap_id, &mut buf)?;
            self.handshake_mut()?.read_message(&buf[..n], &mut []).map_err(|_| ViError::IO)?;

            sys_heartbeat(HEARTBEAT_MS);
            let n = self.handshake_mut()?.write_message(&[], &mut buf).map_err(|_| ViError::IO)?;
            tcp_write_msg(net, cap_id, &buf[..n])?;
        }

        // Finalize: move the handshake out of the Box and into a TransportState.
        let old = core::mem::replace(&mut self.phase, Phase::Finalizing);
        let hs = match old { Phase::Handshake(h) => *h, _ => return Err(ViError::IO) };
        let ts = TransportState::new(hs).map_err(|_| ViError::IO)?;
        self.phase = Phase::Transport(Box::new(ts));
        Ok(())
    }

    /// Encrypt and send `plaintext` as a length-prefixed Noise transport record.
    pub fn send(&mut self, net: &mut NetRef, plaintext: &[u8]) -> ViResult<()> {
        let mut out = [0u8; 4096 + NOISE_TAG];
        let n = match &mut self.phase {
            Phase::Transport(ts) => ts.send(plaintext, &mut out).map_err(|_| ViError::IO)?,
            _ => return Err(ViError::NotSupported),
        };
        tcp_write_msg(net, self.cap_id, &out[..n])
    }

    /// Receive and decrypt one Noise transport record into `out`.
    pub fn recv(&mut self, net: &mut NetRef, out: &mut [u8]) -> ViResult<usize> {
        let mut buf = [0u8; 4096 + NOISE_TAG];
        let n = tcp_read_msg(net, self.cap_id, &mut buf)?;
        match &mut self.phase {
            Phase::Transport(ts) => ts.receive(&buf[..n], out).map_err(|_| ViError::IO),
            _ => Err(ViError::NotSupported),
        }
    }

    fn handshake_mut(&mut self) -> ViResult<&mut Hs> {
        match &mut self.phase {
            Phase::Handshake(hs) => Ok(hs.as_mut()),
            _ => Err(ViError::IO),
        }
    }
}

// ── ConnectionPool ─────────────────────────────────────────────────────────────

/// Bounded pool of active Noise sessions (K = MAX_SESSIONS = 4).
pub struct ConnectionPool {
    sessions: [Option<NoiseSession>; MAX_SESSIONS],
    stamps: [u64; MAX_SESSIONS],
    clock: u64,
}

impl ConnectionPool {
    pub const fn new() -> Self {
        Self {
            sessions: [const { None }; MAX_SESSIONS],
            stamps: [0; MAX_SESSIONS],
            clock: 0,
        }
    }

    /// Insert a session. Evicts LRU slot if the pool is full.
    /// Returns the slot index.
    pub fn insert(&mut self, session: NoiseSession) -> usize {
        self.clock += 1;
        let slot = self.sessions.iter().position(|s| s.is_none()).unwrap_or_else(|| {
            self.stamps.iter().enumerate().min_by_key(|&(_, t)| t).map(|(i, _)| i).unwrap()
        });
        self.sessions[slot] = Some(session);
        self.stamps[slot] = self.clock;
        slot
    }

    pub fn get_mut(&mut self, slot: usize) -> Option<&mut NoiseSession> {
        self.sessions.get_mut(slot)?.as_mut()
    }

    /// Remove session by TCP cap_id (e.g. on TcpClose / peer reset).
    pub fn remove_by_cap(&mut self, cap_id: u32) {
        for i in 0..MAX_SESSIONS {
            if self.sessions[i].as_ref().map(|s| s.cap_id) == Some(cap_id) {
                self.sessions[i] = None;
                self.stamps[i] = 0;
            }
        }
    }

    pub fn len(&self) -> usize {
        self.sessions.iter().filter(|s| s.is_some()).count()
    }
}

// ── TCP framing helpers ────────────────────────────────────────────────────────

/// Send `msg` prefixed with a 2-byte LE length over the net cell's TCP socket.
fn tcp_write_msg(net: &mut NetRef, cap_id: u32, msg: &[u8]) -> ViResult<()> {
    let len_bytes = (msg.len() as u16).to_le_bytes();
    let mut resp = [0u8; api::ipc::IPC_BUF_SIZE];
    net.call::<NetRequest, NetResponse>(
        &NetRequest::TcpSend { cap_id, data: &len_bytes }, &mut resp,
    ).map_err(|_| ViError::IO)?;
    net.call::<NetRequest, NetResponse>(
        &NetRequest::TcpSend { cap_id, data: msg }, &mut resp,
    ).map_err(|_| ViError::IO)?;
    Ok(())
}

/// Receive one length-prefixed message from the net cell's TCP socket.
fn tcp_read_msg(net: &mut NetRef, cap_id: u32, buf: &mut [u8]) -> ViResult<usize> {
    let mut resp = [0u8; api::ipc::IPC_BUF_SIZE];
    // Read 2-byte length header.
    let hdr = match net.call::<NetRequest, NetResponse>(
        &NetRequest::TcpRecv { cap_id, buf_len: 2 }, &mut resp,
    ).map_err(|_| ViError::IO)? {
        NetResponse::Data(d) => d,
        _ => return Err(ViError::IO),
    };
    if hdr.len() < 2 { return Err(ViError::IO); }
    let msg_len = u16::from_le_bytes([hdr[0], hdr[1]]) as usize;
    if msg_len > buf.len() { return Err(ViError::IO); }

    // Read payload — may arrive in one TcpRecv (smoltcp buffers it).
    let payload = match net.call::<NetRequest, NetResponse>(
        &NetRequest::TcpRecv { cap_id, buf_len: msg_len as u32 }, &mut resp,
    ).map_err(|_| ViError::IO)? {
        NetResponse::Data(d) => d,
        _ => return Err(ViError::IO),
    };
    let n = payload.len().min(msg_len);
    buf[..n].copy_from_slice(&payload[..n]);
    Ok(n)
}
