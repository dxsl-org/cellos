//! SSH-2.0 session state machine.
//!
//! Each phase accepts exactly the messages RFC 4253/4252 allow there; anything
//! else is answered with `SSH_MSG_DISCONNECT` and the session closes:
//!
//! | phase            | accepted                                   |
//! |------------------|--------------------------------------------|
//! | `KexInit`        | `KEXINIT`                                  |
//! | `EcdhInit`       | `KEX_ECDH_INIT`                            |
//! | `WaitNewKeys`    | `NEWKEYS`                                  |
//! | `ServiceRequest` | `SERVICE_REQUEST "ssh-userauth"`           |
//! | `UserAuth`       | `USERAUTH_REQUEST`                         |
//! | `ActiveSession`  | `CHANNEL_*`, `GLOBAL_REQUEST`              |
//!
//! After NEWKEYS, `IGNORE`/`DEBUG`/`UNIMPLEMENTED` are skipped in any phase
//! (RFC 4253 §11) and a peer `DISCONNECT` closes. Before NEWKEYS nothing extra
//! is admitted, which also pins both packet sequence numbers at 3 when the
//! cipher switches.

#![forbid(unsafe_code)]

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;
use ed25519_compact::KeyPair;

use crate::auth::{Auth, AuthReply, DISCONNECT_PROTOCOL_ERROR, DISCONNECT_SERVICE_NOT_AVAILABLE};
use crate::cipher::CipherMode;
use crate::connection::Channel;
use crate::kex::{Entropy, KexState, SERVER_VERSION};
use crate::ssh_packet::{
    PacketReader, PacketWriter, SSH_MSG_DEBUG, SSH_MSG_DISCONNECT, SSH_MSG_IGNORE, SSH_MSG_KEXINIT,
    SSH_MSG_KEX_ECDH_INIT, SSH_MSG_NEWKEYS, SSH_MSG_SERVICE_ACCEPT, SSH_MSG_SERVICE_REQUEST,
    SSH_MSG_UNIMPLEMENTED, SSH_MSG_USERAUTH_REQUEST, SSH_MSG_USERAUTH_SUCCESS,
};

/// RFC 4253 §4.2: the identification line, CR LF included, is at most 255 bytes.
pub const MAX_VERSION_LINE: usize = 255;
pub(crate) const DISCONNECT_KEY_EXCHANGE_FAILED: u32 = 3;

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum SessionPhase {
    VersionExchange,
    KexInit,
    EcdhInit,
    WaitNewKeys,
    ServiceRequest,
    UserAuth,
    ActiveSession,
    Closed,
}

pub struct SshSession {
    pub phase: SessionPhase,
    pub(crate) kex: KexState,
    pub(crate) cipher: CipherMode,
    pub(crate) rx_buf: Vec<u8>,
    auth: Auth,
    pub(crate) channel: Channel,
}

impl SshSession {
    /// A connection served with `host_key`, fresh secrets from `entropy`, and
    /// `authorized` as the only keys that may log in.
    pub fn new(host_key: KeyPair, entropy: Entropy, authorized: Vec<[u8; 32]>) -> Self {
        Self {
            phase: SessionPhase::VersionExchange,
            kex: KexState::new(host_key, entropy),
            cipher: CipherMode::new_plaintext(),
            rx_buf: Vec::with_capacity(4096),
            auth: Auth::new(authorized),
            channel: Channel::new(),
        }
    }

    /// Bytes to send as soon as the TCP connection is established.
    pub fn initial_outbound(&self) -> &'static [u8] {
        SERVER_VERSION
    }

    pub fn is_authenticated(&self) -> bool {
        self.phase == SessionPhase::ActiveSession
    }

    /// Consume bytes from the socket; return the bytes to send back.
    pub fn process_incoming(&mut self, data: &[u8]) -> Vec<u8> {
        self.rx_buf.extend_from_slice(data);
        let mut out = Vec::new();
        while self.phase != SessionPhase::Closed {
            if self.phase == SessionPhase::VersionExchange {
                if !self.version_exchange(&mut out) {
                    break;
                }
                continue;
            }
            let replies = match self.cipher.unpack_inbound(&self.rx_buf) {
                Ok(Some((payload, consumed))) => {
                    self.rx_buf.drain(..consumed);
                    self.dispatch(&payload)
                }
                Ok(None) => break,
                Err(()) => self.disconnect(DISCONNECT_PROTOCOL_ERROR, "packet rejected"),
            };
            for payload in replies {
                out.extend_from_slice(&self.cipher.pack_outbound(&payload));
            }
        }
        out
    }

    /// Handle one decrypted payload; return the payloads to send.
    pub(crate) fn dispatch(&mut self, payload: &[u8]) -> Vec<Vec<u8>> {
        let Some((&msg, body)) = payload.split_first() else {
            return self.disconnect(DISCONNECT_PROTOCOL_ERROR, "empty packet");
        };
        let keyed = !matches!(
            self.phase,
            SessionPhase::KexInit | SessionPhase::EcdhInit | SessionPhase::WaitNewKeys
        );
        match (self.phase, msg) {
            (_, SSH_MSG_DISCONNECT) => {
                self.close("peer disconnected");
                Vec::new()
            }
            (_, SSH_MSG_IGNORE | SSH_MSG_DEBUG | SSH_MSG_UNIMPLEMENTED) if keyed => Vec::new(),
            (SessionPhase::KexInit, SSH_MSG_KEXINIT) => {
                self.kex.client_kexinit = payload.to_vec();
                self.phase = SessionPhase::EcdhInit;
                Vec::new()
            }
            (SessionPhase::EcdhInit, SSH_MSG_KEX_ECDH_INIT) => self.ecdh_init(body),
            (SessionPhase::WaitNewKeys, SSH_MSG_NEWKEYS) => self.new_keys(),
            (SessionPhase::ServiceRequest, SSH_MSG_SERVICE_REQUEST) => {
                if PacketReader::new(body).read_string() != Some(&b"ssh-userauth"[..]) {
                    return self.disconnect(DISCONNECT_SERVICE_NOT_AVAILABLE, "unknown service");
                }
                self.phase = SessionPhase::UserAuth;
                let mut w = PacketWriter::new();
                w.write_byte(SSH_MSG_SERVICE_ACCEPT);
                w.write_string(b"ssh-userauth");
                vec![w.as_slice().to_vec()]
            }
            (SessionPhase::UserAuth, SSH_MSG_USERAUTH_REQUEST) => {
                match self.auth.handle(&self.kex.session_id, body) {
                    AuthReply::Continue(reply) => vec![reply],
                    AuthReply::Success => {
                        crate::log("[sshd] publickey login accepted");
                        self.phase = SessionPhase::ActiveSession;
                        vec![vec![SSH_MSG_USERAUTH_SUCCESS]]
                    }
                    AuthReply::Disconnect(reason, why) => self.disconnect(reason, why),
                }
            }
            (SessionPhase::ActiveSession, _) => match self.connection_message(msg, body) {
                Ok(replies) => replies,
                Err(why) => self.disconnect(DISCONNECT_PROTOCOL_ERROR, why),
            },
            _ => self.disconnect(
                DISCONNECT_PROTOCOL_ERROR,
                "message not allowed in this phase",
            ),
        }
    }

    /// `SSH_MSG_DISCONNECT` (RFC 4253 §11.1) and close.
    pub(crate) fn disconnect(&mut self, reason: u32, why: &'static str) -> Vec<Vec<u8>> {
        self.close(why);
        let mut w = PacketWriter::new();
        w.write_byte(SSH_MSG_DISCONNECT);
        w.write_u32(reason);
        w.write_string(why.as_bytes());
        w.write_string(b"");
        vec![w.as_slice().to_vec()]
    }

    pub(crate) fn close(&mut self, why: &str) {
        crate::log2("[sshd] closing session: ", why);
        self.phase = SessionPhase::Closed;
    }
}

#[cfg(test)]
mod tests;
