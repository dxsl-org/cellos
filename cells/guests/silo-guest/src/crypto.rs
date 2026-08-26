// SPDX-License-Identifier: MPL-2.0
//! Development-only P-256 custody for the Stage-2 Silo guest.

use p256::ecdsa::signature::hazmat::PrehashSigner;
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

/// Opaque one-time initialized guest state.
pub struct SiloState {
    key: Option<SigningKey>,
}

/// Result of one purpose-specific mailbox operation.
pub enum CryptoResult {
    /// One-time initialization completed and exposes only the public point.
    Ready([u8; 65]),
    /// Fixed-width normalized P-256 signature (`r || s`).
    Signature([u8; 64]),
    /// Bounded guest failure code.
    Fault(u8),
}

const _: () = assert!(core::mem::size_of::<CryptoResult>() <= 72);

impl SiloState {
    /// Create state that rejects signing until one-time initialization succeeds.
    pub const fn uninit() -> Self {
        Self { key: None }
    }

    /// Initialize exactly once and zero the caller's transient seed on every path.
    pub fn initialize_once(&mut self, seed: &mut [u8; 32]) -> CryptoResult {
        if self.key.is_some() {
            seed.fill(0);
            return CryptoResult::Fault(0x02);
        }
        let parsed = SigningKey::from_bytes(seed.as_slice().into());
        seed.fill(0);
        match parsed {
            Ok(key) => {
                let point = VerifyingKey::from(&key).to_encoded_point(false);
                let mut public = [0u8; 65];
                public.copy_from_slice(point.as_bytes());
                self.key = Some(key);
                CryptoResult::Ready(public)
            }
            Err(_) => CryptoResult::Fault(0x01),
        }
    }

    /// Sign exactly the TLS 1.3 client CertificateVerify transcript construction.
    pub fn sign_tls13_client_certificate_verify(
        &self,
        transcript_hash: [u8; 32],
    ) -> CryptoResult {
        let Some(key) = self.key.as_ref() else {
            return CryptoResult::Fault(0x10);
        };
        let mut hash = Sha256::new();
        hash.update([0x20; 64]);
        hash.update(b"TLS 1.3, client CertificateVerify\0");
        hash.update(transcript_hash);
        let digest: [u8; 32] = hash.finalize().into();
        let signature: Signature = match key.sign_prehash(&digest) {
            Ok(signature) => signature,
            Err(_) => return CryptoResult::Fault(0x11),
        };
        let normalized = signature.normalize_s().unwrap_or(signature);
        let mut raw = [0u8; 64];
        raw.copy_from_slice(&normalized.to_bytes());
        CryptoResult::Signature(raw)
    }
}
