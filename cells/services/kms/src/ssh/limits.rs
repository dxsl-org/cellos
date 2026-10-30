//! K5-local bounds. Everything already frozen by the ABI is re-used from
//! `types::kms`; the parser-side bounds (KDF salt/rounds, stored wrapped size)
//! live in `parser.rs` so that file stays `core`-only.

use types::kms::{KMS_SSH_IMPORT_MAX_LEN, KMS_SSH_USERNAME_MAX};

/// Consecutive `SshUnlockFailed` results after which the key is locked (O-5).
pub(crate) const FAIL_LIMIT: u8 = 5;
/// Principals whose replay floor is remembered; the oldest is evicted (O-4).
pub(crate) const FLOOR_SLOTS: usize = 4;
/// Retained staging arena: one maximum-size import.
pub(crate) const STAGING_LEN: usize = KMS_SSH_IMPORT_MAX_LEN;
/// Zero-filled stack region written after every unlock and sign.
pub(crate) const SCRUB_LEN: usize = 16 * 1024;
/// RFC 4252 section 7 input without the username (`143 + n`).
pub(crate) const SIGN_INPUT_FIXED: usize = 143;
/// Longest reconstructed userauth input (`n = 31`).
pub(crate) const SIGN_INPUT_MAX: usize = SIGN_INPUT_FIXED + KMS_SSH_USERNAME_MAX;
