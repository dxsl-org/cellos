use p256::ecdsa::signature::hazmat::PrehashVerifier;
use p256::ecdsa::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use types::kms::{assemble_relay_csr, canonical_relay_cri, der_ecdsa_signature, RELAY_CSR_MAX_LEN};

use super::RelaySignError;

/// Canonical CSR assembled inside KMS after self-verifying the provider proof.
pub(crate) struct AssembledCsr {
    pub bytes: [u8; RELAY_CSR_MAX_LEN],
    pub len: usize,
}

impl AssembledCsr {
    pub fn sha256(&self) -> [u8; 32] {
        Sha256::digest(&self.bytes[..self.len]).into()
    }
}

/// Verify the provider's raw `r||s` proof against KMS's *own* canonical CRI
/// reconstruction and assemble the bounded PKCS#10 CSR.
///
/// The provider signed its independent reconstruction of the same frozen
/// profile; verification therefore proves both sides built byte-identical
/// CRIs over the fresh non-exportable key before anything is published.
pub(crate) fn verify_and_assemble_csr(
    hostname: &[u8],
    spki_sec1: &[u8; 65],
    raw_signature: [u8; 64],
) -> Result<AssembledCsr, RelaySignError> {
    let (cri, cri_len) =
        canonical_relay_cri(hostname, spki_sec1).ok_or(RelaySignError::InvalidRequest)?;
    let digest = Sha256::digest(&cri[..cri_len]);
    let signature = Signature::from_slice(&raw_signature).map_err(|_| RelaySignError::Failure)?;
    let normalized = signature.normalize_s().unwrap_or(signature);
    let verifying_key =
        VerifyingKey::from_sec1_bytes(spki_sec1).map_err(|_| RelaySignError::Failure)?;
    verifying_key
        .verify_prehash(&digest, &normalized)
        .map_err(|_| RelaySignError::Failure)?;
    let mut raw = [0u8; 64];
    raw.copy_from_slice(normalized.to_bytes().as_slice());
    let (sig_der, sig_len) = der_ecdsa_signature(&raw);
    let (bytes, len) = assemble_relay_csr(&cri, cri_len, &sig_der, sig_len)
        .ok_or(RelaySignError::InvalidRequest)?;
    Ok(AssembledCsr { bytes, len })
}
