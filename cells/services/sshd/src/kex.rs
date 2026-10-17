//! SSH-2.0 Key Exchange (RFC 4253, RFC 8731) and Host Key Signing.

#![forbid(unsafe_code)]

extern crate alloc;
use alloc::vec::Vec;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};
use ed25519_compact::{KeyPair, Seed};

use crate::ssh_packet::{PacketWriter, SSH_MSG_KEXINIT, SSH_MSG_KEX_ECDH_REPLY, SSH_MSG_NEWKEYS};

pub const SERVER_VERSION: &[u8] = b"SSH-2.0-Cellos_1.0\r\n";
pub const SERVER_VERSION_RAW: &[u8] = b"SSH-2.0-Cellos_1.0";

pub struct KexState {
    pub client_version: Vec<u8>,
    pub server_version: Vec<u8>,
    pub client_kexinit: Vec<u8>,
    pub server_kexinit: Vec<u8>,
    pub host_keypair: KeyPair,
    pub server_secret: Option<StaticSecret>,
    pub server_public: Option<PublicKey>,
    pub session_id: Vec<u8>,
    pub shared_secret_k: Vec<u8>,
    pub exchange_hash_h: Vec<u8>,
}

impl KexState {
    pub fn new() -> Self {
        // Deterministic or seeded host key for Cellos
        let seed = Seed::from_slice(&[
            0x43, 0x65, 0x6c, 0x6c, 0x6f, 0x73, 0x53, 0x53,
            0x48, 0x44, 0x65, 0x76, 0x48, 0x6f, 0x73, 0x74,
            0x4b, 0x65, 0x79, 0x53, 0x65, 0x65, 0x64, 0x32,
            0x30, 0x32, 0x36, 0x31, 0x30, 0x31, 0x37, 0x01,
        ]).unwrap();
        let host_keypair = KeyPair::from_seed(seed);

        Self {
            client_version: Vec::new(),
            server_version: SERVER_VERSION_RAW.to_vec(),
            client_kexinit: Vec::new(),
            server_kexinit: Vec::new(),
            host_keypair,
            server_secret: None,
            server_public: None,
            session_id: Vec::new(),
            shared_secret_k: Vec::new(),
            exchange_hash_h: Vec::new(),
        }
    }

    /// Build Server Host Key blob: string "ssh-ed25519" || string pk
    pub fn host_key_blob(&self) -> Vec<u8> {
        let mut w = PacketWriter::new();
        w.write_string(b"ssh-ed25519");
        w.write_string(self.host_keypair.pk.as_ref());
        w.as_slice().to_vec()
    }

    /// Create Server KEXINIT payload
    pub fn make_server_kexinit(&mut self) -> Vec<u8> {
        let mut w = PacketWriter::new();
        w.write_byte(SSH_MSG_KEXINIT);
        // 16 bytes cookie
        w.write_raw(&[
            0x10, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
            0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x01,
        ]);
        w.write_name_list(&["curve25519-sha256", "curve25519-sha256@libssh.org"]);
        w.write_name_list(&["ssh-ed25519"]);
        w.write_name_list(&["chacha20-poly1305@openssh.com", "none"]);
        w.write_name_list(&["chacha20-poly1305@openssh.com", "none"]);
        w.write_name_list(&["none"]);
        w.write_name_list(&["none"]);
        w.write_name_list(&["none"]);
        w.write_name_list(&["none"]);
        w.write_string(b""); // languages client to server
        w.write_string(b""); // languages server to client
        w.write_bool(false); // first_kex_packet_follows
        w.write_u32(0);      // reserved

        let payload = w.as_slice().to_vec();
        self.server_kexinit = payload.clone();
        payload
    }

    /// Handle client ECDH init, calculate shared secret K, exchange hash H, and reply
    pub fn handle_ecdh_init(&mut self, client_pubkey_bytes: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
        if client_pubkey_bytes.len() != 32 {
            return None;
        }
        let mut client_arr = [0u8; 32];
        client_arr.copy_from_slice(client_pubkey_bytes);
        let client_pub = PublicKey::from(client_arr);

        // Server ephemeral keys
        let server_secret = StaticSecret::from([
            0x71, 0x82, 0x93, 0xa4, 0xb5, 0xc6, 0xd7, 0xe8,
            0x09, 0x1a, 0x2b, 0x3c, 0x4d, 0x5e, 0x6f, 0x70,
            0x81, 0x92, 0xa3, 0xb4, 0xc5, 0xd6, 0xe7, 0xf8,
            0x19, 0x2a, 0x3b, 0x4c, 0x5d, 0x6e, 0x7f, 0x80,
        ]);
        let server_pub = PublicKey::from(&server_secret);
        let shared_secret = server_secret.diffie_hellman(&client_pub);
        let k_bytes = shared_secret.as_bytes();

        let host_key_blob = self.host_key_blob();

        // Calculate Exchange Hash H = SHA256(V_C || V_S || I_C || I_S || K_S || Q_C || Q_S || K)
        let mut hasher = Sha256::new();
        // V_C (string)
        hasher.update(&(self.client_version.len() as u32).to_be_bytes());
        hasher.update(&self.client_version);
        // V_S (string)
        hasher.update(&(self.server_version.len() as u32).to_be_bytes());
        hasher.update(&self.server_version);
        // I_C (string)
        hasher.update(&(self.client_kexinit.len() as u32).to_be_bytes());
        hasher.update(&self.client_kexinit);
        // I_S (string)
        hasher.update(&(self.server_kexinit.len() as u32).to_be_bytes());
        hasher.update(&self.server_kexinit);
        // K_S (string)
        hasher.update(&(host_key_blob.len() as u32).to_be_bytes());
        hasher.update(&host_key_blob);
        // Q_C (string)
        hasher.update(&(client_pubkey_bytes.len() as u32).to_be_bytes());
        hasher.update(client_pubkey_bytes);
        // Q_S (string)
        hasher.update(&(server_pub.as_bytes().len() as u32).to_be_bytes());
        hasher.update(server_pub.as_bytes());
        // K (mpint)
        let mut mpint_w = PacketWriter::new();
        mpint_w.write_mpint(k_bytes);
        hasher.update(mpint_w.as_slice());

        let h_hash = hasher.finalize();
        let h_bytes = h_hash.as_slice();

        // Sign H with host key Ed25519
        let sig = self.host_keypair.sk.sign(h_bytes, None);
        let mut sig_w = PacketWriter::new();
        sig_w.write_string(b"ssh-ed25519");
        sig_w.write_string(sig.as_ref());
        let sig_blob = sig_w.as_slice().to_vec();

        // Build SSH_MSG_KEX_ECDH_REPLY
        let mut reply = PacketWriter::new();
        reply.write_byte(SSH_MSG_KEX_ECDH_REPLY);
        reply.write_string(&host_key_blob);
        reply.write_string(server_pub.as_bytes());
        reply.write_string(&sig_blob);

        if self.session_id.is_empty() {
            self.session_id = h_bytes.to_vec();
        }
        self.shared_secret_k = k_bytes.to_vec();
        self.exchange_hash_h = h_bytes.to_vec();

        // Also build SSH_MSG_NEWKEYS
        let mut newkeys = PacketWriter::new();
        newkeys.write_byte(SSH_MSG_NEWKEYS);

        Some((reply.as_slice().to_vec(), newkeys.as_slice().to_vec()))
    }

    /// Derive key letter X ('A'..'F') per RFC 4253 §7.2
    pub fn derive_key(&self, letter: u8, key_len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(key_len);
        let mut mpint_w = PacketWriter::new();
        mpint_w.write_mpint(&self.shared_secret_k);

        // K1 = HASH(K || H || X || session_id)
        let mut hasher = Sha256::new();
        hasher.update(mpint_w.as_slice());
        hasher.update(&self.exchange_hash_h);
        hasher.update(&[letter]);
        hasher.update(&self.session_id);
        let h1 = hasher.finalize();
        out.extend_from_slice(&h1);

        // If key_len > 32, extend with K2 = HASH(K || H || K1)
        while out.len() < key_len {
            let mut hasher2 = Sha256::new();
            hasher2.update(mpint_w.as_slice());
            hasher2.update(&self.exchange_hash_h);
            hasher2.update(&out[out.len() - 32..]);
            let h2 = hasher2.finalize();
            out.extend_from_slice(&h2);
        }
        out.truncate(key_len);
        out
    }
}
