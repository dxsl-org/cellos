//! Embedded guest artifact admission before any VM syscall.

use sha2::{Digest, Sha256};

/// Maximum flat image length before the final mailbox page.
pub use crate::layout::MAX_GUEST_BYTES;

/// Artifact admission failure, always detected before VM creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactError {
    /// Development packaging was not selected or emitted no bytes.
    Empty,
    /// Flat bytes would overlap the mailbox page.
    Oversized,
    /// Embedded bytes do not match build-generated integrity metadata.
    DigestMismatch,
}

#[cfg(feature = "development-silo-provider")]
mod packaged {
    pub static BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/silo-guest.bin"));
    include!(concat!(env!("OUT_DIR"), "/silo-guest-digest.rs"));
}

/// Return the admitted development guest bytes.
///
/// Empty, oversized, and digest-mismatched artifacts are rejected before the
/// caller is permitted to issue `CreateVm`.
pub fn admitted_guest() -> Result<&'static [u8], ArtifactError> {
    #[cfg(not(feature = "development-silo-provider"))]
    let (bytes, expected): (&'static [u8], [u8; 32]) = (&[], [0; 32]);
    #[cfg(feature = "development-silo-provider")]
    let (bytes, expected) = (packaged::BYTES, packaged::GUEST_SHA256);
    validate(bytes, expected)
}

fn validate(bytes: &[u8], expected: [u8; 32]) -> Result<&[u8], ArtifactError> {
    if bytes.is_empty() {
        return Err(ArtifactError::Empty);
    }
    if bytes.len() > MAX_GUEST_BYTES {
        return Err(ArtifactError::Oversized);
    }
    let actual: [u8; 32] = Sha256::digest(bytes).into();
    if actual != expected {
        return Err(ArtifactError::DigestMismatch);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_oversized_and_tampered_artifacts_are_rejected() {
        static OVERSIZED: [u8; MAX_GUEST_BYTES + 1] = [1; MAX_GUEST_BYTES + 1];
        assert_eq!(validate(&[], [0; 32]), Err(ArtifactError::Empty));
        assert_eq!(validate(&OVERSIZED, [0; 32]), Err(ArtifactError::Oversized));
        assert_eq!(validate(&[1], [0; 32]), Err(ArtifactError::DigestMismatch));
    }

    #[test]
    fn matching_integrity_metadata_admits_nonempty_bytes() {
        let bytes = [1u8, 2, 3];
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        assert_eq!(validate(&bytes, digest), Ok(bytes.as_slice()));
    }
}
