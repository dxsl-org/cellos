//! Transport handshake steps (RFC 4253 §4.2, §7, §8) of [`SshSession`].

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use crate::auth::DISCONNECT_PROTOCOL_ERROR;
use crate::cipher::CipherMode;
use crate::session::{SessionPhase, SshSession, DISCONNECT_KEY_EXCHANGE_FAILED, MAX_VERSION_LINE};
use crate::ssh_packet::PacketReader;

/// Packets each side sends before the cipher switch (KEXINIT, ECDH, NEWKEYS).
/// The session admits nothing else before NEWKEYS, so this is exact.
const KEX_PACKETS: u32 = 3;

impl SshSession {
    /// Consume the client identification line. `false` while more bytes are needed.
    pub(crate) fn version_exchange(&mut self, out: &mut Vec<u8>) -> bool {
        let Some(pos) = self.rx_buf.iter().position(|&b| b == b'\n') else {
            if self.rx_buf.len() >= MAX_VERSION_LINE {
                self.close("version line too long");
            }
            return false;
        };
        let line = &self.rx_buf[..pos];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if pos + 1 > MAX_VERSION_LINE || !line.starts_with(b"SSH-2.0-") {
            self.close("bad version line");
            return false;
        }
        self.kex.client_version = line.to_vec();
        self.rx_buf.drain(..=pos);
        match self.kex.make_server_kexinit() {
            Some(kexinit) => {
                out.extend_from_slice(&self.cipher.pack_outbound(&kexinit));
                self.phase = SessionPhase::KexInit;
            }
            None => self.close("kernel entropy unavailable"),
        }
        true
    }

    /// `KEX_ECDH_INIT`: reply with ECDH_REPLY and our NEWKEYS, or disconnect.
    pub(crate) fn ecdh_init(&mut self, body: &[u8]) -> Vec<Vec<u8>> {
        let mut r = PacketReader::new(body);
        let Some(q_c) = r.read_string().filter(|_| r.is_at_end()) else {
            return self.disconnect(DISCONNECT_PROTOCOL_ERROR, "malformed ECDH init");
        };
        match self.kex.handle_ecdh_init(q_c) {
            Ok((reply, newkeys)) => {
                self.phase = SessionPhase::WaitNewKeys;
                vec![reply, newkeys]
            }
            Err(e) => self.disconnect(DISCONNECT_KEY_EXCHANGE_FAILED, e.reason()),
        }
    }

    /// The client's NEWKEYS: both directions switch to chacha20-poly1305.
    pub(crate) fn new_keys(&mut self) -> Vec<Vec<u8>> {
        let c2s_key = self.kex.derive_key(b'C', 64);
        let s2c_key = self.kex.derive_key(b'D', 64);
        self.cipher = CipherMode::new_chacha20(&c2s_key, &s2c_key, KEX_PACKETS, KEX_PACKETS);
        self.phase = SessionPhase::ServiceRequest;
        Vec::new()
    }
}
