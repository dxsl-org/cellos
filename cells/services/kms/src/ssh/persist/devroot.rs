//! The T1 development root: a public build-time constant, not a secret.
//!
//! The record MAC key is derived from [`DEV_ROOT_LABEL`], which is in the
//! `kms` binary and in the repository. Anyone holding the disk and either of
//! those can recompute it and forge a consistent record pair. It is a format
//! and corruption check with domain separation, not authentication. What
//! protects the stored key at rest is the user's passphrase.

use blake2::{Blake2s256, Digest};

/// Also an artifact marker for `scripts/check-ssh-dev-persist-exclusion.py`.
pub(crate) const DEV_ROOT_LABEL: &[u8] = b"CELLOS-KSSH-T1-DEV-ROOT-v1 PUBLIC CONSTANT NOT A SECRET";

/// `BLAKE2s-256(DEV_ROOT_LABEL)`.
pub(crate) fn mac_key() -> [u8; 32] {
    Blake2s256::digest(DEV_ROOT_LABEL).into()
}
