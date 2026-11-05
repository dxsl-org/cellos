//! SSH-2.0 Key Exchange (RFC 4253, RFC 8731) and Host Key Signing.
//!
//! Every secret here is fresh per connection: the KEXINIT cookie and the x25519
//! scalar come from the injected [`Entropy`] source (the kernel's in the cell),
//! and a failed draw aborts the exchange — there is no fallback value.

#![forbid(unsafe_code)]

extern crate alloc;
use alloc::vec::Vec;
use ed25519_compact::KeyPair;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::ssh_packet::{PacketWriter, SSH_MSG_KEXINIT, SSH_MSG_KEX_ECDH_REPLY, SSH_MSG_NEWKEYS};

pub const SERVER_VERSION: &[u8] = b"SSH-2.0-Cellos_1.0\r\n";
pub const SERVER_VERSION_RAW: &[u8] = b"SSH-2.0-Cellos_1.0";

/// Fill the whole buffer with cryptographic random bytes, or return `false`.
pub type Entropy = fn(&mut [u8]) -> bool;

/// Kernel entropy: every 64-byte chunk (the syscall's per-call cap) must be
/// filled completely, otherwise the draw failed.
#[cfg(target_os = "none")]
pub fn kernel_entropy(buf: &mut [u8]) -> bool {
    buf.chunks_mut(64)
        .all(|c| ostd::syscall::sys_get_random(c) == c.len())
}

/// Why a key exchange was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KexError {
    /// The entropy source could not supply a fresh scalar.
    Entropy,
    /// `Q_C` is not a 32-byte x25519 point.
    BadPublicKey,
    /// `Q_C` is a small-order point: the shared secret would be all-zero and
    /// known to anyone (RFC 7748 §6.1, RFC 8731 §3).
    NonContributory,
}

impl KexError {
    /// The `SSH_MSG_DISCONNECT` description.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Entropy => "kernel entropy unavailable",
            Self::BadPublicKey => "bad x25519 public key",
            Self::NonContributory => "non-contributory x25519 key",
        }
    }
}

pub struct KexState {
    pub client_version: Vec<u8>,
    pub client_kexinit: Vec<u8>,
    server_kexinit: Vec<u8>,
    host_keypair: KeyPair,
    entropy: Entropy,
    /// H of the first (only) exchange; RFC 4253 §7.2 keeps it for the session.
    pub session_id: [u8; 32],
    shared_secret_k: [u8; 32],
}

impl KexState {
    pub fn new(host_keypair: KeyPair, entropy: Entropy) -> Self {
        Self {
            client_version: Vec::new(),
            client_kexinit: Vec::new(),
            server_kexinit: Vec::new(),
            host_keypair,
            entropy,
            session_id: [0; 32],
            shared_secret_k: [0; 32],
        }
    }

    /// Server KEXINIT with a fresh random cookie; `None` if entropy failed.
    pub fn make_server_kexinit(&mut self) -> Option<Vec<u8>> {
        let mut cookie = [0u8; 16];
        if !(self.entropy)(&mut cookie) {
            return None;
        }
        let mut w = PacketWriter::new();
        w.write_byte(SSH_MSG_KEXINIT);
        w.write_raw(&cookie);
        w.write_name_list(&["curve25519-sha256", "curve25519-sha256@libssh.org"]);
        w.write_name_list(&["ssh-ed25519"]);
        // The only cipher implemented; `none` is never offered.
        w.write_name_list(&["chacha20-poly1305@openssh.com"]);
        w.write_name_list(&["chacha20-poly1305@openssh.com"]);
        // MAC is implicit in chacha20-poly1305; compression is not supported.
        w.write_name_list(&["none"]);
        w.write_name_list(&["none"]);
        w.write_name_list(&["none"]);
        w.write_name_list(&["none"]);
        w.write_string(b""); // languages client to server
        w.write_string(b""); // languages server to client
        w.write_bool(false); // first_kex_packet_follows
        w.write_u32(0); // reserved

        let payload = w.as_slice().to_vec();
        self.server_kexinit = payload.clone();
        Some(payload)
    }

    /// Handle client ECDH init: fresh x25519 scalar, shared secret K, exchange
    /// hash H signed by the host key. Returns (ECDH_REPLY, NEWKEYS) payloads.
    pub fn handle_ecdh_init(
        &mut self,
        client_pubkey_bytes: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>), KexError> {
        let client_arr: [u8; 32] = client_pubkey_bytes
            .try_into()
            .map_err(|_| KexError::BadPublicKey)?;
        let client_pub = PublicKey::from(client_arr);

        let mut scalar = [0u8; 32];
        if !(self.entropy)(&mut scalar) {
            return Err(KexError::Entropy);
        }
        let server_secret = StaticSecret::from(scalar);
        let server_pub = PublicKey::from(&server_secret);
        let shared_secret = server_secret.diffie_hellman(&client_pub);
        if !shared_secret.was_contributory() {
            return Err(KexError::NonContributory);
        }
        let k_bytes = shared_secret.as_bytes();
        let host_key_blob = host_key_blob(&self.host_keypair);

        // H = SHA256(V_C || V_S || I_C || I_S || K_S || Q_C || Q_S || K)
        let mut hasher = Sha256::new();
        for string in [
            &self.client_version[..],
            SERVER_VERSION_RAW,
            &self.client_kexinit[..],
            &self.server_kexinit[..],
            &host_key_blob[..],
            client_pubkey_bytes,
            &server_pub.as_bytes()[..],
        ] {
            hasher.update((string.len() as u32).to_be_bytes());
            hasher.update(string);
        }
        let mut mpint_w = PacketWriter::new();
        mpint_w.write_mpint(k_bytes);
        hasher.update(mpint_w.as_slice());
        let h: [u8; 32] = hasher.finalize().into();

        let sig = self.host_keypair.sk.sign(h, None);
        let mut sig_w = PacketWriter::new();
        sig_w.write_string(b"ssh-ed25519");
        sig_w.write_string(sig.as_ref());

        let mut reply = PacketWriter::new();
        reply.write_byte(SSH_MSG_KEX_ECDH_REPLY);
        reply.write_string(&host_key_blob);
        reply.write_string(server_pub.as_bytes());
        reply.write_string(sig_w.as_slice());

        // Re-keying is not supported, so this is always the first exchange.
        self.session_id = h;
        self.shared_secret_k = *k_bytes;
        Ok((reply.as_slice().to_vec(), [SSH_MSG_NEWKEYS].to_vec()))
    }

    /// Derive key letter X ('A'..'F') per RFC 4253 §7.2
    pub fn derive_key(&self, letter: u8, key_len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(key_len);
        let mut mpint_w = PacketWriter::new();
        mpint_w.write_mpint(&self.shared_secret_k);

        // K1 = HASH(K || H || X || session_id); H == session_id (single KEX).
        let mut hasher = Sha256::new();
        hasher.update(mpint_w.as_slice());
        hasher.update(self.session_id);
        hasher.update([letter]);
        hasher.update(self.session_id);
        out.extend_from_slice(&hasher.finalize());

        // Kn = HASH(K || H || K1 || … || Kn-1)
        while out.len() < key_len {
            let mut hasher2 = Sha256::new();
            hasher2.update(mpint_w.as_slice());
            hasher2.update(self.session_id);
            hasher2.update(&out);
            out.extend_from_slice(&hasher2.finalize());
        }
        out.truncate(key_len);
        out
    }
}

/// `string "ssh-ed25519" || string pk`.
pub fn host_key_blob(keypair: &KeyPair) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_string(b"ssh-ed25519");
    w.write_string(keypair.pk.as_ref());
    w.as_slice().to_vec()
}
