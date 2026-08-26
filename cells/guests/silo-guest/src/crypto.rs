// SPDX-License-Identifier: MPL-2.0
//! Development-only P-256 custody for the Stage-2 Silo guest.

use p256::ecdsa::signature::hazmat::PrehashSigner;
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use types::kms::{canonical_relay_cri, validate_hostname, CRI_MAX_LEN};

/// Opaque one-time initialized guest state.
pub struct SiloState {
    key: Option<SigningKey>,
    enrollment: Option<(u64, SigningKey)>,
}

/// Result of one purpose-specific mailbox operation.
pub enum CryptoResult {
    /// One-time initialization or key creation; exposes only the public point.
    Ready([u8; 65]),
    /// Fixed-width normalized P-256 signature (`r || s`).
    Signature([u8; 64]),
    /// Explicit destruction acknowledged; no payload.
    Ack,
    /// Destruction confirmed the named key was already absent.
    Absent,
    /// Bounded guest failure code.
    Fault(u8),
}

const _: () = assert!(core::mem::size_of::<CryptoResult>() <= 72);

impl SiloState {
    /// Create state that rejects signing until one-time initialization succeeds.
    pub const fn uninit() -> Self {
        Self {
            key: None,
            enrollment: None,
        }
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
    pub fn sign_tls13_client_certificate_verify(&self, transcript_hash: [u8; 32]) -> CryptoResult {
        let Some(key) = self.key.as_ref() else {
            return CryptoResult::Fault(0x10);
        };
        let digest = tls_digest(transcript_hash);
        sign_low_s(key, &digest)
    }

    /// Create the fresh non-exportable key for one pending generation.
    ///
    /// Only one pending enrollment key may exist at a time; it must be
    /// destroyed explicitly before another generation can enroll. The scalar
    /// is derived inside the guest from custody material that never leaves,
    /// then its transient bytes are zeroized on every path.
    pub fn create_enrollment_key(
        &mut self,
        pending_generation: u64,
        nonce: &mut [u8; 32],
    ) -> CryptoResult {
        if self.key.is_none() {
            nonce.fill(0);
            core::hint::black_box(nonce);
            return CryptoResult::Fault(0x10);
        }
        if pending_generation == 0
            || nonce.iter().all(|byte| *byte == 0)
            || self.enrollment.is_some()
        {
            nonce.fill(0);
            core::hint::black_box(nonce);
            return CryptoResult::Fault(0x20);
        }
        let mut material = Sha256::new();
        material.update(b"silo-enrollment-key-v1");
        material.update(pending_generation.to_le_bytes());
        material.update(nonce.as_slice());
        material.update(self.key.as_ref().expect("checked").to_bytes());
        let mut seed: [u8; 32] = material.finalize().into();
        nonce.fill(0);
        core::hint::black_box(nonce);
        let mut parsed = SigningKey::from_bytes((&seed).into());
        let mut retries = 0;
        while parsed.is_err() && retries < 8 {
            seed = Sha256::digest(seed).into();
            parsed = SigningKey::from_bytes((&seed).into());
            retries += 1;
        }
        match parsed {
            Ok(enrollment_key) => {
                seed.fill(0);
                core::hint::black_box(&seed);
                let point = VerifyingKey::from(&enrollment_key).to_encoded_point(false);
                let mut public = [0u8; 65];
                public.copy_from_slice(point.as_bytes());
                self.enrollment = Some((pending_generation, enrollment_key));
                CryptoResult::Ready(public)
            }
            Err(_) => {
                seed.fill(0);
                core::hint::black_box(&seed);
                CryptoResult::Fault(0x21)
            }
        }
    }

    /// Reconstruct the canonical CRI independently and sign only that.
    ///
    /// The hostname comes from the request; everything else (SPKI, profile
    /// shape) comes from the frozen builder shared with KMS. Raw `r||s` only.
    pub fn sign_enrollment_cri(
        &self,
        pending_generation: u64,
        hostname: &[u8],
    ) -> CryptoResult {
        let Some((generation, enrollment_key)) = self.enrollment.as_ref() else {
            return CryptoResult::Fault(0x22);
        };
        if *generation != pending_generation || !validate_hostname(hostname) {
            return CryptoResult::Fault(0x23);
        }
        let point = VerifyingKey::from(enrollment_key).to_encoded_point(false);
        let mut sec1 = [0u8; 65];
        sec1.copy_from_slice(point.as_bytes());
        let Some((cri, cri_len)) = canonical_relay_cri(hostname, &sec1) else {
            return CryptoResult::Fault(0x24);
        };
        debug_assert!(cri_len <= CRI_MAX_LEN);
        let digest: [u8; 32] = Sha256::digest(&cri[..cri_len]).into();
        sign_low_s(enrollment_key, &digest)
    }

    /// Destroy the pending enrollment key explicitly; abort, commit, and
    /// revocation paths all funnel through this operation.
    pub fn destroy_enrollment_key(&mut self, pending_generation: u64) -> CryptoResult {
        let Some((generation, _)) = self.enrollment.as_ref() else {
            return CryptoResult::Absent;
        };
        if *generation != pending_generation {
            return CryptoResult::Fault(0x25);
        }
        let (_, key) = self.enrollment.take().expect("generation checked");
        // Zeroize the scalar's transient representation before dropping it.
        let mut scalar = key.to_bytes();
        scalar.fill(0);
        core::hint::black_box(&scalar);
        CryptoResult::Ack
    }
}

fn tls_digest(transcript_hash: [u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update([0x20; 64]);
    hash.update(b"TLS 1.3, client CertificateVerify\0");
    hash.update(transcript_hash);
    hash.finalize().into()
}

fn sign_low_s(key: &SigningKey, digest: &[u8; 32]) -> CryptoResult {
    let Ok(signature): Result<Signature, _> = key.sign_prehash(digest) else {
        return CryptoResult::Fault(0x11);
    };
    let normalized = signature.normalize_s().unwrap_or(signature);
    let mut raw = [0u8; 64];
    raw.copy_from_slice(&normalized.to_bytes());
    CryptoResult::Signature(raw)
}

impl SiloState {
    /// Atomically promote the pending key to the active signer and retire
    /// the previous active key. On success the new public point is returned.
    pub fn promote_enrollment_key(&mut self, pending_generation: u64) -> CryptoResult {
        let Some((generation, _)) = self.enrollment else {
            return CryptoResult::Fault(0x28);
        };
        if generation != pending_generation {
            return CryptoResult::Fault(0x29);
        }
        let (_, promoted) = self.enrollment.take().expect("checked");
        let point = VerifyingKey::from(&promoted).to_encoded_point(false);
        let mut public = [0u8; 65];
        public.copy_from_slice(point.as_bytes());
        // Zeroize the retired active signer before replacing it.
        if let Some(old) = self.key.take() {
            let mut scalar = old.to_bytes();
            for byte in scalar.iter_mut() {
                *byte = 0;
            }
            core::hint::black_box(&scalar);
        }
        self.key = Some(promoted);
        CryptoResult::Ready(public)
    }
}
