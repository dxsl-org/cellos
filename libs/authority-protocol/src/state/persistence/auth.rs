use super::ProtectedAuthorityRecord;
use sha2::{Digest, Sha256};

pub const PROTECTED_RECORD_MAX: usize = 1024;

impl ProtectedAuthorityRecord {
    /// Hash the exact canonical persistence bytes; no Rust layout is authenticated.
    pub fn authentication_digest(&self) -> [u8; 32] {
        let mut encoded = [0u8; PROTECTED_RECORD_MAX];
        let length = self
            .encode_canonical(&mut encoded)
            .expect("protected authority record fits its fixed maximum");
        Sha256::digest(&encoded[..length]).into()
    }
}
