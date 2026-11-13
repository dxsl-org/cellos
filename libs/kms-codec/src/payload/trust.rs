// SPDX-License-Identifier: MPL-2.0
//! Fixed wire payloads of the SSH host-trust store (opcodes 25-29).
//!
//! A query names a host (up to 253 bytes), an algorithm and a complete SSH key blob
//! (up to 4 KiB), which do not fit the 112-byte payload, so it is staged like an SSH
//! import: `BeginTrustQuery` declares the kind, port, field lengths and the SHA-256 of
//! the staged bytes `host || algorithm || key`; `TrustQueryChunk` delivers them in the
//! canonical 96-byte chunks ([`crate::ssh_chunk_len`]); `CommitTrustQuery` verifies
//! count and digest and answers a [`TrustOutcomePayload`]. Chunk, chunk ack, Begin
//! response, Commit request and Abort request are the SSH transfer payloads
//! ([`super::SshChunkRequestPayload`], [`super::SshChunkAckPayload`],
//! [`super::SshTransferBeginResponsePayload`], [`super::SshImportCommitRequestPayload`],
//! [`super::SshAbortRequestPayload`]).
//!
//! The outcome byte is one of `wire::ipc::ssh_trust` (0-7); this crate does not depend on
//! `wire`, so only its range is checked here.

use super::{put_u16, put_u64, read_32, read_u16, read_u64};
use crate::KmsTrustQueryKind;

/// Longest host identifier (a DNS name). Frozen.
pub const KMS_TRUST_HOST_MAX: usize = 253;
/// Longest algorithm name. Frozen.
pub const KMS_TRUST_ALGORITHM_MAX: usize = 64;
/// Longest SSH key blob. Frozen.
pub const KMS_TRUST_KEY_MAX: usize = 4096;
/// Longest staged query (`host || algorithm || key`).
pub const KMS_TRUST_QUERY_MAX: usize =
    KMS_TRUST_HOST_MAX + KMS_TRUST_ALGORITHM_MAX + KMS_TRUST_KEY_MAX;
/// Highest `wire::ipc::ssh_trust` outcome (`PENDING`).
pub const KMS_TRUST_OUTCOME_MAX: u8 = 7;
/// `wire::ipc::ssh_trust::UNKNOWN`: the outcome that accompanies an issued challenge.
pub const KMS_TRUST_OUTCOME_UNKNOWN: u8 = 1;

fn is_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

/// An `ObjectRefV1` (`id`, `generation`) as two little-endian u64 words.
fn read_ref(bytes: &[u8], at: usize) -> (u64, u64) {
    (read_u64(bytes, at), read_u64(bytes, at + 8))
}

fn put_ref(out: &mut [u8], at: usize, value: (u64, u64)) {
    put_u64(out, at, value.0);
    put_u64(out, at + 8, value.1);
}

/// `BeginTrustQuery` request.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustQueryBeginRequestPayload {
    pub kind: KmsTrustQueryKind,
    /// `1..=64`.
    pub algorithm_len: u8,
    /// Nonzero.
    pub port: u16,
    /// `1..=253`.
    pub host_len: u16,
    /// `1..=4096`.
    pub key_len: u16,
    /// Challenge: the caller's terminal-session epoch. Lookup: zero.
    pub epoch: u64,
    /// Challenge: the caller's own terminal session (`id`, `generation`), never zero.
    /// Lookup: zero.
    pub session: (u64, u64),
    /// SHA-256 of `host || algorithm || key`; never all zero.
    pub query_sha256: [u8; 32],
}

impl TrustQueryBeginRequestPayload {
    pub const LEN: usize = 64;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0] = self.kind as u8;
        out[1] = self.algorithm_len;
        put_u16(&mut out, 2, self.port);
        put_u16(&mut out, 4, self.host_len);
        put_u16(&mut out, 6, self.key_len);
        put_u64(&mut out, 8, self.epoch);
        put_ref(&mut out, 16, self.session);
        out[32..64].copy_from_slice(&self.query_sha256);
        out
    }

    /// Strict decode: every length within its bound, a nonzero port and digest, and a
    /// session (and epoch) exactly when the kind is a challenge.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        let kind = KmsTrustQueryKind::try_from(bytes[0]).ok()?;
        let algorithm_len = bytes[1];
        let port = read_u16(bytes, 2);
        let host_len = read_u16(bytes, 4);
        let key_len = read_u16(bytes, 6);
        let epoch = read_u64(bytes, 8);
        let session = read_ref(bytes, 16);
        let query_sha256 = read_32(bytes, 32);
        let no_session = session == (0, 0);
        let session_ok = match kind {
            KmsTrustQueryKind::Lookup => no_session && epoch == 0,
            KmsTrustQueryKind::Challenge => !no_session,
        };
        if port == 0
            || !(1..=KMS_TRUST_ALGORITHM_MAX).contains(&usize::from(algorithm_len))
            || !(1..=KMS_TRUST_HOST_MAX).contains(&usize::from(host_len))
            || !(1..=KMS_TRUST_KEY_MAX).contains(&usize::from(key_len))
            || is_zero(&query_sha256)
            || !session_ok
        {
            return None;
        }
        Some(Self {
            kind,
            algorithm_len,
            port,
            host_len,
            key_len,
            epoch,
            session,
            query_sha256,
        })
    }

    /// Length of the staged bytes `host || algorithm || key`.
    pub fn total_len(&self) -> u32 {
        u32::from(self.host_len) + u32::from(self.algorithm_len) + u32::from(self.key_len)
    }
}

/// `CommitTrustQuery` and `CommitTrustChallenge` response.
///
/// `outcome` is one `wire::ipc::ssh_trust` code. A challenge issued by `CommitTrustQuery`
/// sets `challenged`, carries the outcome Unknown and both nonzero refs: the input-issued
/// `confirmation` the caller begins on its terminal, and the KMS-issued `challenge` it
/// commits. Every other reply has `challenged == 0` and zero refs.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustOutcomePayload {
    pub outcome: u8,
    pub challenged: u8,
    pub reserved: [u8; 6],
    pub confirmation: (u64, u64),
    pub challenge: (u64, u64),
}

impl TrustOutcomePayload {
    pub const LEN: usize = 40;

    /// A reply carrying only `outcome`.
    pub const fn outcome(outcome: u8) -> Self {
        Self {
            outcome,
            challenged: 0,
            reserved: [0; 6],
            confirmation: (0, 0),
            challenge: (0, 0),
        }
    }

    /// An issued challenge.
    pub const fn challenged(confirmation: (u64, u64), challenge: (u64, u64)) -> Self {
        Self {
            outcome: KMS_TRUST_OUTCOME_UNKNOWN,
            challenged: 1,
            reserved: [0; 6],
            confirmation,
            challenge,
        }
    }

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0] = self.outcome;
        out[1] = self.challenged;
        out[2..8].copy_from_slice(&self.reserved);
        put_ref(&mut out, 8, self.confirmation);
        put_ref(&mut out, 24, self.challenge);
        out
    }

    /// Strict decode: one of the two canonical shapes above.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || !is_zero(&bytes[2..8]) {
            return None;
        }
        let (outcome, confirmation, challenge) =
            (bytes[0], read_ref(bytes, 8), read_ref(bytes, 24));
        match bytes[1] {
            0 if outcome <= KMS_TRUST_OUTCOME_MAX && is_zero(&bytes[8..40]) => {
                Some(Self::outcome(outcome))
            }
            1 if outcome == KMS_TRUST_OUTCOME_UNKNOWN
                && confirmation != (0, 0)
                && challenge != (0, 0) =>
            {
                Some(Self::challenged(confirmation, challenge))
            }
            _ => None,
        }
    }
}

/// `CommitTrustChallenge` request: the KMS-issued challenge ref.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustChallengeCommitRequestPayload {
    /// Nonzero.
    pub challenge: (u64, u64),
}

impl TrustChallengeCommitRequestPayload {
    pub const LEN: usize = 16;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_ref(&mut out, 0, self.challenge);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        let challenge = read_ref(bytes, 0);
        (challenge != (0, 0)).then_some(Self { challenge })
    }
}

const _: () = assert!(TrustQueryBeginRequestPayload::LEN == 1 + 1 + 2 + 2 + 2 + 8 + 16 + 32);
const _: () = assert!(TrustOutcomePayload::LEN == 1 + 1 + 6 + 16 + 16);
const _: () = assert!(TrustChallengeCommitRequestPayload::LEN == 16);
const _: () = assert!(KMS_TRUST_QUERY_MAX <= crate::KMS_SSH_IMPORT_MAX_LEN);
