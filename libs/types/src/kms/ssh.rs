// SPDX-License-Identifier: MPL-2.0
//! Frozen bounds, validators, and canonical chunk arithmetic for the single
//! KMS-custodied OpenSSH ed25519 identity (opcodes 15-24).
//!
//! These definitions are shared by KMS, its clients, and tests so that one
//! implementation of every bound exists. Nothing here selects a key or an
//! algorithm: a KMS store holds exactly one SSH identity and the signing
//! request names only typed protocol state.

/// Longest admin-chosen KEY_ID name (`[A-Za-z0-9_-]`, 1..=64). Frozen.
pub const KMS_SSH_KEY_NAME_MAX: usize = 64;
/// Longest SSH userauth username (`[A-Za-z0-9._-]`, 1..=31). Frozen.
pub const KMS_SSH_USERNAME_MAX: usize = 31;
/// SSH session identifier length (curve25519-sha256 exchange hash). Frozen.
pub const KMS_SSH_SESSION_ID_LEN: usize = 32;
/// Raw ed25519 public key length. Frozen.
pub const KMS_SSH_PUBLIC_KEY_LEN: usize = 32;
/// Raw ed25519 signature length (`R || S`). Frozen.
pub const KMS_SSH_SIGNATURE_LEN: usize = 64;
/// Largest OpenSSH key blob (binary, not armored) one import may carry. Frozen.
pub const KMS_SSH_IMPORT_MAX_LEN: usize = 64 * 1024;
/// Data bytes per transfer chunk: the 112-byte payload minus a 16-byte header.
/// Frozen.
pub const KMS_SSH_CHUNK_LEN: usize = 96;
/// Longest passphrase staged for one unlock. Frozen.
pub const KMS_SSH_SECRET_MAX: usize = 1024;

/// Validate an admin-chosen key name: 1..=64 bytes of `[A-Za-z0-9_-]`.
pub fn validate_ssh_key_name(name: &[u8]) -> bool {
    !name.is_empty()
        && name.len() <= KMS_SSH_KEY_NAME_MAX
        && name
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// Validate an SSH userauth username: 1..=31 bytes of `[A-Za-z0-9._-]`.
pub fn validate_ssh_username(username: &[u8]) -> bool {
    !username.is_empty()
        && username.len() <= KMS_SSH_USERNAME_MAX
        && username
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Canonical number of chunks for a transfer of `total_len` bytes:
/// `ceil(total_len / 96)`. Zero bytes need zero chunks (never a valid transfer).
pub const fn ssh_chunk_count(total_len: u32) -> u32 {
    total_len.div_ceil(KMS_SSH_CHUNK_LEN as u32)
}

/// Canonical length of chunk `chunk_index` of a `total_len`-byte transfer:
/// 96 for every chunk but the last, the remainder for the last, and `None` for
/// an index past the end. One transfer has exactly one valid chunk sequence.
pub const fn ssh_chunk_len(total_len: u32, chunk_index: u32) -> Option<u16> {
    let count = ssh_chunk_count(total_len);
    if chunk_index >= count {
        return None;
    }
    if chunk_index + 1 < count {
        Some(KMS_SSH_CHUNK_LEN as u16)
    } else {
        Some((total_len - KMS_SSH_CHUNK_LEN as u32 * chunk_index) as u16)
    }
}
