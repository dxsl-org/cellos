// SPDX-License-Identifier: MPL-2.0
//! Fixed wire payloads for the single KMS-custodied OpenSSH ed25519 identity.
//!
//! Opcodes 15-24 are purpose-specific: there is no generic upload, generic
//! sign, algorithm, key-handle, or raw-message field. The signing request
//! carries only typed userauth state (session id, username) plus the expected
//! public key; KMS reconstructs the RFC 4252 section 7 input itself.
//!
//! Every payload has a fixed `LEN`. Decoders are strict: exact length, zero
//! reserved bytes, a length byte followed by zero padding for variable fields,
//! validated charsets, and the value bounds frozen in [`crate::kms`]'s SSH
//! constants. Stateful rules (floors, handle ownership, order) belong to the
//! service, not to these codecs. A zero transfer handle in a request is
//! structurally valid and is the service's `SshHandleInvalid`.

use super::{put_u16, put_u32, put_u64, read_32, read_u16, read_u32, read_u64};
use crate::kms::ssh::{
    ssh_chunk_count, validate_ssh_key_name, validate_ssh_username, KMS_SSH_CHUNK_LEN,
    KMS_SSH_IMPORT_MAX_LEN, KMS_SSH_KEY_NAME_MAX, KMS_SSH_PUBLIC_KEY_LEN, KMS_SSH_SECRET_MAX,
    KMS_SSH_SESSION_ID_LEN, KMS_SSH_SIGNATURE_LEN, KMS_SSH_USERNAME_MAX,
};
use crate::kms::{
    KmsCapabilityReadiness, KmsCustodyPosture, KmsKeyAlgorithm, KmsProviderKind, KmsSshKeyCustody,
};

fn is_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

/// Decode a `len` byte plus zero-padded name field. `allow_empty` admits
/// `len == 0` (the leaf status query); otherwise the name must validate.
fn decode_name(len: u8, field: &[u8], allow_empty: bool) -> Option<[u8; KMS_SSH_KEY_NAME_MAX]> {
    let len = len as usize;
    if field.len() != KMS_SSH_KEY_NAME_MAX || len > KMS_SSH_KEY_NAME_MAX {
        return None;
    }
    if !is_zero(&field[len..])
        || !((len == 0 && allow_empty) || validate_ssh_key_name(&field[..len]))
    {
        return None;
    }
    let mut name = [0u8; KMS_SSH_KEY_NAME_MAX];
    name[..len].copy_from_slice(&field[..len]);
    Some(name)
}

fn decode_username(len: u8, field: &[u8]) -> Option<[u8; KMS_SSH_USERNAME_MAX]> {
    let len = len as usize;
    if field.len() != KMS_SSH_USERNAME_MAX || len > KMS_SSH_USERNAME_MAX {
        return None;
    }
    if !is_zero(&field[len..]) || !validate_ssh_username(&field[..len]) {
        return None;
    }
    let mut username = [0u8; KMS_SSH_USERNAME_MAX];
    username[..len].copy_from_slice(&field[..len]);
    Some(username)
}

/// `BeginSshKeyImport` request: admin-chosen name, blob length, blob SHA-256.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshImportBeginRequestPayload {
    /// Nonzero and strictly above the caller's floor (enforced by the service).
    pub request_id: u64,
    /// Complete OpenSSH blob length, `1..=KMS_SSH_IMPORT_MAX_LEN`.
    pub total_len: u32,
    pub name_len: u8,
    pub reserved: [u8; 3],
    /// SHA-256 of the complete binary OpenSSH blob; never all zero.
    pub blob_sha256: [u8; 32],
    pub name: [u8; KMS_SSH_KEY_NAME_MAX],
}

impl SshImportBeginRequestPayload {
    pub const LEN: usize = 112;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.request_id);
        put_u32(&mut out, 8, self.total_len);
        out[12] = self.name_len;
        out[13..16].copy_from_slice(&self.reserved);
        out[16..48].copy_from_slice(&self.blob_sha256);
        out[48..112].copy_from_slice(&self.name);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || !is_zero(&bytes[13..16]) {
            return None;
        }
        let request_id = read_u64(bytes, 0);
        let total_len = read_u32(bytes, 8);
        let blob_sha256 = read_32(bytes, 16);
        if request_id == 0
            || total_len == 0
            || total_len as usize > KMS_SSH_IMPORT_MAX_LEN
            || is_zero(&blob_sha256)
        {
            return None;
        }
        Some(Self {
            request_id,
            total_len,
            name_len: bytes[12],
            reserved: [0; 3],
            blob_sha256,
            name: decode_name(bytes[12], &bytes[48..112], false)?,
        })
    }

    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }
}

/// `BeginSshKeyImport` and `BeginSshUnlock` response: the one-shot transfer
/// handle and the canonical chunk plan.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshTransferBeginResponsePayload {
    /// Nonzero, bound to the opening principal; not a secret.
    pub handle: u64,
    /// Import: blob length. Unlock: secret length.
    pub total_len: u32,
    /// `ceil(total_len / 96)`.
    pub chunk_count: u32,
    /// Always [`KMS_SSH_CHUNK_LEN`].
    pub chunk_capacity: u16,
    pub reserved: [u8; 6],
}

impl SshTransferBeginResponsePayload {
    pub const LEN: usize = 24;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.handle);
        put_u32(&mut out, 8, self.total_len);
        put_u32(&mut out, 12, self.chunk_count);
        put_u16(&mut out, 16, self.chunk_capacity);
        out[18..24].copy_from_slice(&self.reserved);
        out
    }

    /// Strict decode: nonzero handle, total within the import bound, the
    /// canonical chunk count, the frozen capacity, and zero reserved bytes.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || !is_zero(&bytes[18..24]) {
            return None;
        }
        let handle = read_u64(bytes, 0);
        let total_len = read_u32(bytes, 8);
        if handle == 0
            || total_len == 0
            || total_len as usize > KMS_SSH_IMPORT_MAX_LEN
            || read_u32(bytes, 12) != ssh_chunk_count(total_len)
            || read_u16(bytes, 16) as usize != KMS_SSH_CHUNK_LEN
        {
            return None;
        }
        Some(Self {
            handle,
            total_len,
            chunk_count: read_u32(bytes, 12),
            chunk_capacity: read_u16(bytes, 16),
            reserved: [0; 6],
        })
    }
}

/// `ImportSshKeyChunk` and `SshUnlockChunk` request: one ordered chunk.
///
/// Canonical chunking: every chunk but the last carries exactly 96 bytes, so
/// one blob has exactly one valid chunk sequence. That rule is checked by the
/// service against the recorded total; the codec checks the per-chunk bound.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshChunkRequestPayload {
    pub handle: u64,
    pub chunk_index: u32,
    /// `1..=96`.
    pub chunk_len: u16,
    pub reserved: u16,
    /// Data, zero after `chunk_len`.
    pub chunk: [u8; KMS_SSH_CHUNK_LEN],
}

impl SshChunkRequestPayload {
    pub const LEN: usize = 112;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.handle);
        put_u32(&mut out, 8, self.chunk_index);
        put_u16(&mut out, 12, self.chunk_len);
        put_u16(&mut out, 14, self.reserved);
        out[16..112].copy_from_slice(&self.chunk);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || read_u16(bytes, 14) != 0 {
            return None;
        }
        let chunk_len = read_u16(bytes, 12) as usize;
        if chunk_len == 0 || chunk_len > KMS_SSH_CHUNK_LEN || !is_zero(&bytes[16 + chunk_len..]) {
            return None;
        }
        Some(Self {
            handle: read_u64(bytes, 0),
            chunk_index: read_u32(bytes, 8),
            chunk_len: read_u16(bytes, 12),
            reserved: 0,
            chunk: bytes[16..112].try_into().expect("fixed chunk body"),
        })
    }

    pub fn data(&self) -> &[u8] {
        &self.chunk[..self.chunk_len as usize]
    }
}

/// `ImportSshKeyChunk` and `SshUnlockChunk` response: running transfer state.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshChunkAckPayload {
    pub handle: u64,
    pub next_chunk_index: u32,
    /// Cumulative bytes received, `1..=KMS_SSH_IMPORT_MAX_LEN`.
    pub received_len: u32,
}

impl SshChunkAckPayload {
    pub const LEN: usize = 16;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.handle);
        put_u32(&mut out, 8, self.next_chunk_index);
        put_u32(&mut out, 12, self.received_len);
        out
    }

    /// Strict decode: nonzero handle, cumulative bytes within bounds, and the
    /// next index that canonical chunking implies (`ceil(received / 96)`).
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        let handle = read_u64(bytes, 0);
        let next_chunk_index = read_u32(bytes, 8);
        let received_len = read_u32(bytes, 12);
        if handle == 0
            || received_len == 0
            || received_len as usize > KMS_SSH_IMPORT_MAX_LEN
            || next_chunk_index != ssh_chunk_count(received_len)
        {
            return None;
        }
        Some(Self {
            handle,
            next_chunk_index,
            received_len,
        })
    }
}

/// `CommitSshKeyImport` request: echoes what Begin recorded and what arrived.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshImportCommitRequestPayload {
    pub handle: u64,
    pub chunk_count: u32,
    pub reserved: u32,
    pub blob_sha256: [u8; 32],
}

impl SshImportCommitRequestPayload {
    pub const LEN: usize = 48;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.handle);
        put_u32(&mut out, 8, self.chunk_count);
        put_u32(&mut out, 12, self.reserved);
        out[16..48].copy_from_slice(&self.blob_sha256);
        out
    }

    /// Mismatching count or digest is the service's order or integrity error,
    /// so only the reserved word is checked structurally.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || read_u32(bytes, 12) != 0 {
            return None;
        }
        Some(Self {
            handle: read_u64(bytes, 0),
            chunk_count: read_u32(bytes, 8),
            reserved: 0,
            blob_sha256: read_32(bytes, 16),
        })
    }
}

/// `AbortSshKeyImport` and `AbortSshUnlock` request: destroy the named transfer.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshAbortRequestPayload {
    pub handle: u64,
}

impl SshAbortRequestPayload {
    pub const LEN: usize = 8;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.handle);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        Some(Self {
            handle: read_u64(bytes, 0),
        })
    }
}

/// `CommitSshKeyImport` and `GetSshKeyStatus` response: public SSH leaf facts.
///
/// Independent of C2C X25519 and Relay P-256 readiness. Carries no private,
/// wrapped, or passphrase material.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshKeyStatusPayload {
    pub algorithm: KmsKeyAlgorithm,
    pub readiness: KmsCapabilityReadiness,
    pub provider: KmsProviderKind,
    pub posture: KmsCustodyPosture,
    /// 1 only when the named key is stored; 0 for the leaf query.
    pub key_present: u8,
    pub custody: KmsSshKeyCustody,
    /// Stored keys; at most one (exactly one identity per store).
    pub key_count: u16,
    /// Bumps on every import and delete; resets at boot at T0.
    pub store_revision: u64,
    /// All zero when `key_present == 0`.
    pub public_key: [u8; KMS_SSH_PUBLIC_KEY_LEN],
    /// SHA-256 of the 51-byte SSH wire public key blob; all zero when
    /// `key_present == 0`.
    pub fingerprint_sha256: [u8; 32],
}

impl SshKeyStatusPayload {
    pub const LEN: usize = 80;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0] = self.algorithm as u8;
        out[1] = self.readiness as u8;
        out[2] = self.provider as u8;
        out[3] = self.posture as u8;
        out[4] = self.key_present;
        out[5] = self.custody as u8;
        put_u16(&mut out, 6, self.key_count);
        put_u64(&mut out, 8, self.store_revision);
        out[16..48].copy_from_slice(&self.public_key);
        out[48..80].copy_from_slice(&self.fingerprint_sha256);
        out
    }

    /// Strict decode: the SSH algorithm, known enum values, and the presence
    /// invariants (no key means no custody, public key, or fingerprint; a
    /// present key means a custody mode and exactly one stored key).
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        let algorithm = KmsKeyAlgorithm::try_from(bytes[0]).ok()?;
        let custody = KmsSshKeyCustody::try_from(bytes[5]).ok()?;
        let key_count = read_u16(bytes, 6);
        let public_key = read_32(bytes, 16);
        let fingerprint_sha256 = read_32(bytes, 48);
        if algorithm != KmsKeyAlgorithm::SshEd25519 || key_count > 1 {
            return None;
        }
        match bytes[4] {
            0 if custody == KmsSshKeyCustody::None
                && is_zero(&public_key)
                && is_zero(&fingerprint_sha256) => {}
            1 if custody != KmsSshKeyCustody::None && key_count == 1 => {}
            _ => return None,
        }
        Some(Self {
            algorithm,
            readiness: KmsCapabilityReadiness::try_from(bytes[1]).ok()?,
            provider: KmsProviderKind::try_from(bytes[2]).ok()?,
            posture: KmsCustodyPosture::try_from(bytes[3]).ok()?,
            key_present: bytes[4],
            custody,
            key_count,
            store_revision: read_u64(bytes, 8),
            public_key,
            fingerprint_sha256,
        })
    }
}

/// `GetSshKeyStatus` request: empty name is the leaf query, else the one key.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshStatusRequestPayload {
    /// `0` (leaf status) or `1..=64`.
    pub name_len: u8,
    pub name: [u8; KMS_SSH_KEY_NAME_MAX],
}

impl SshStatusRequestPayload {
    pub const LEN: usize = 1 + KMS_SSH_KEY_NAME_MAX;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0] = self.name_len;
        out[1..].copy_from_slice(&self.name);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        Some(Self {
            name_len: bytes[0],
            name: decode_name(bytes[0], &bytes[1..], true)?,
        })
    }

    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }
}

/// `DeleteSshKey` request: the name and the public key the admin believes is
/// stored. A mismatch is `SshKeyNotFound`, which prevents a delete racing a
/// delete-and-reimport.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshDeleteRequestPayload {
    pub request_id: u64,
    pub public_key: [u8; KMS_SSH_PUBLIC_KEY_LEN],
    pub name_len: u8,
    pub name: [u8; KMS_SSH_KEY_NAME_MAX],
}

impl SshDeleteRequestPayload {
    pub const LEN: usize = 8 + KMS_SSH_PUBLIC_KEY_LEN + 1 + KMS_SSH_KEY_NAME_MAX;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.request_id);
        out[8..40].copy_from_slice(&self.public_key);
        out[40] = self.name_len;
        out[41..].copy_from_slice(&self.name);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        let request_id = read_u64(bytes, 0);
        if request_id == 0 {
            return None;
        }
        Some(Self {
            request_id,
            public_key: read_32(bytes, 8),
            name_len: bytes[40],
            name: decode_name(bytes[40], &bytes[41..], false)?,
        })
    }

    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }
}

/// `BeginSshUnlock` request: open a one-shot staged secret for the wrapped
/// identity. Valid only for a `PassphraseWrapped` key.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshUnlockBeginRequestPayload {
    pub request_id: u64,
    pub public_key: [u8; KMS_SSH_PUBLIC_KEY_LEN],
    /// `1..=KMS_SSH_SECRET_MAX`.
    pub secret_len: u16,
    pub reserved0: u16,
    pub reserved1: u32,
}

impl SshUnlockBeginRequestPayload {
    pub const LEN: usize = 48;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.request_id);
        out[8..40].copy_from_slice(&self.public_key);
        put_u16(&mut out, 40, self.secret_len);
        put_u16(&mut out, 42, self.reserved0);
        put_u32(&mut out, 44, self.reserved1);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || read_u16(bytes, 42) != 0 || read_u32(bytes, 44) != 0 {
            return None;
        }
        let request_id = read_u64(bytes, 0);
        let secret_len = read_u16(bytes, 40);
        if request_id == 0 || secret_len == 0 || secret_len as usize > KMS_SSH_SECRET_MAX {
            return None;
        }
        Some(Self {
            request_id,
            public_key: read_32(bytes, 8),
            secret_len,
            reserved0: 0,
            reserved1: 0,
        })
    }
}

/// `SignSshUserauth` request: typed userauth state only.
///
/// The service, method, signature algorithm, and public key blob are not
/// carried. KMS reconstructs the `143 + username_len` byte RFC 4252 section 7
/// input; the public key here is an expected-key confirmation that must equal
/// the single stored identity.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshSignRequestPayload {
    pub request_id: u64,
    pub public_key: [u8; KMS_SSH_PUBLIC_KEY_LEN],
    /// SSH session identifier; never all zero.
    pub session_id: [u8; KMS_SSH_SESSION_ID_LEN],
    /// `1..=31`.
    pub username_len: u8,
    pub username: [u8; KMS_SSH_USERNAME_MAX],
}

impl SshSignRequestPayload {
    pub const LEN: usize = 104;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.request_id);
        out[8..40].copy_from_slice(&self.public_key);
        out[40..72].copy_from_slice(&self.session_id);
        out[72] = self.username_len;
        out[73..104].copy_from_slice(&self.username);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        let request_id = read_u64(bytes, 0);
        let session_id = read_32(bytes, 40);
        if request_id == 0 || is_zero(&session_id) {
            return None;
        }
        Some(Self {
            request_id,
            public_key: read_32(bytes, 8),
            session_id,
            username_len: bytes[72],
            username: decode_username(bytes[72], &bytes[73..104])?,
        })
    }

    pub fn username(&self) -> &[u8] {
        &self.username[..self.username_len as usize]
    }
}

/// `SignSshUserauth` response: the raw signature and the digest of the exact
/// signed input, so a client can detect a reconstruction disagreement locally.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshSignResponsePayload {
    /// Echo of the request id; nonzero.
    pub request_id: u64,
    /// Raw ed25519 `R || S`; the SSH `string` wrapping is the client's job.
    pub signature: [u8; KMS_SSH_SIGNATURE_LEN],
    /// SHA-256 of the exact signed input.
    pub message_sha256: [u8; 32],
}

impl SshSignResponsePayload {
    pub const LEN: usize = 104;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.request_id);
        out[8..72].copy_from_slice(&self.signature);
        out[72..104].copy_from_slice(&self.message_sha256);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        let request_id = read_u64(bytes, 0);
        if request_id == 0 {
            return None;
        }
        let mut signature = [0u8; KMS_SSH_SIGNATURE_LEN];
        signature.copy_from_slice(&bytes[8..72]);
        Some(Self {
            request_id,
            signature,
            message_sha256: read_32(bytes, 72),
        })
    }
}

const _: () = assert!(SshImportBeginRequestPayload::LEN == 8 + 4 + 1 + 3 + 32 + 64);
const _: () = assert!(SshTransferBeginResponsePayload::LEN == 8 + 4 + 4 + 2 + 6);
const _: () = assert!(SshChunkRequestPayload::LEN == 8 + 4 + 2 + 2 + KMS_SSH_CHUNK_LEN);
const _: () = assert!(SshChunkAckPayload::LEN == 8 + 4 + 4);
const _: () = assert!(SshImportCommitRequestPayload::LEN == 8 + 4 + 4 + 32);
const _: () = assert!(SshAbortRequestPayload::LEN == 8);
const _: () = assert!(SshKeyStatusPayload::LEN == 6 + 2 + 8 + 32 + 32);
const _: () = assert!(SshStatusRequestPayload::LEN == 65);
const _: () = assert!(SshDeleteRequestPayload::LEN == 105);
const _: () = assert!(SshUnlockBeginRequestPayload::LEN == 8 + 32 + 2 + 2 + 4);
const _: () = assert!(SshSignRequestPayload::LEN == 8 + 32 + 32 + 1 + 31);
const _: () = assert!(SshSignResponsePayload::LEN == 8 + 64 + 32);

const _: () = assert!(core::mem::size_of::<SshImportBeginRequestPayload>() == 112);
const _: () = assert!(core::mem::size_of::<SshTransferBeginResponsePayload>() == 24);
const _: () = assert!(core::mem::size_of::<SshChunkRequestPayload>() == 112);
const _: () = assert!(core::mem::size_of::<SshChunkAckPayload>() == 16);
const _: () = assert!(core::mem::size_of::<SshImportCommitRequestPayload>() == 48);
const _: () = assert!(core::mem::size_of::<SshAbortRequestPayload>() == 8);
const _: () = assert!(core::mem::size_of::<SshKeyStatusPayload>() == 80);
const _: () = assert!(core::mem::size_of::<SshStatusRequestPayload>() == 65);
const _: () = assert!(core::mem::size_of::<SshUnlockBeginRequestPayload>() == 48);
const _: () = assert!(core::mem::size_of::<SshSignRequestPayload>() == 104);
const _: () = assert!(core::mem::size_of::<SshSignResponsePayload>() == 104);
// The 105-byte delete request leads with a u64, so `repr(C)` pads its size to
// 112; every payload still fits the 112-byte frame payload.
const _: () = assert!(core::mem::size_of::<SshDeleteRequestPayload>() == 112);
