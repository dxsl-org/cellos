use sha2::{Digest, Sha256};

/// Returns the SHA-256 digest of exactly `data`, without allocation.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(data);
    let mut out = [0; 32];
    out.copy_from_slice(&digest);
    out
}
