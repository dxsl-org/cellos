use p256::ecdsa::signature::hazmat::PrehashVerifier;
use p256::ecdsa::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

use super::RelaySignError;

/// Validate scalar ranges, normalize to low-S, and verify against the exact TLS
/// 1.3 client CertificateVerify message reconstructed inside KMS.
pub(crate) fn normalize_and_verify_tls13_signature(
    transcript_hash: [u8; 32],
    verifying_key_sec1: &[u8; 65],
    raw_signature: [u8; 64],
) -> Result<[u8; 64], RelaySignError> {
    let signature = Signature::from_slice(&raw_signature).map_err(|_| RelaySignError::Failure)?;
    let normalized = signature.normalize_s().unwrap_or(signature);
    let verifying_key = VerifyingKey::from_sec1_bytes(verifying_key_sec1)
        .map_err(|_| RelaySignError::Failure)?;
    let digest = kms_tls_digest(transcript_hash);
    verifying_key
        .verify_prehash(&digest, &normalized)
        .map_err(|_| RelaySignError::Failure)?;
    let mut output = [0u8; 64];
    output.copy_from_slice(normalized.to_bytes().as_slice());
    Ok(output)
}

fn kms_tls_digest(transcript_hash: [u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update([0x20; 64]);
    hash.update(b"TLS 1.3, client CertificateVerify\0");
    hash.update(transcript_hash);
    hash.finalize().into()
}
