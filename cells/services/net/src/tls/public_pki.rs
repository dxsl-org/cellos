//! Public WebPKI for the opt-in QEMU DEV flavor.
//!
//! WebPKI performs path building, CA constraints, EKU and SAN name checks. The
//! crypto adapter below uses only RustCrypto (no platform-dependent ring/C TLS).
//! Every certificate in the selected path must cover the whole authenticated
//! clock window. Trust anchors follow WebPKI semantics: their identity, not
//! their self-signature or certificate dates, establishes trust.

use alloc::boxed::Box;
use core::time::Duration;
use embedded_tls::{
    Aes128GcmSha256, CertificateEntryRef, CertificateRef, CertificateVerifyRef,
    SignatureScheme, TlsError, TlsVerifier,
};
use p256::ecdsa::signature::hazmat::PrehashVerifier;
use pki_types::{
    alg_id, AlgorithmIdentifier, CertificateDer, InvalidSignature, ServerName,
    SignatureVerificationAlgorithm, UnixTime,
};
use rsa::{pkcs1::DecodeRsaPublicKey, traits::PublicKeyParts, Pkcs1v15Sign, RsaPublicKey};
use sha2::{Digest, Sha256, Sha384, Sha512};

/// The only copied certificate is the leaf, needed after its handshake record
/// has been consumed to authenticate the subsequent CertificateVerify message.
/// Chain certificates remain borrowed during path validation.
pub(super) struct PublicVerifier {
    hostname: heapless::String<253>,
    leaf: Option<Box<[u8]>>,
    transcript: Option<Sha256>,
}

impl PublicVerifier {
    pub(super) fn new(hostname: &str) -> Result<Self, TlsError> {
        let mut verifier = Self {
            hostname: heapless::String::new(),
            leaf: None,
            transcript: None,
        };
        verifier.set_hostname_verification(hostname)?;
        Ok(verifier)
    }
}

impl TlsVerifier<Aes128GcmSha256> for PublicVerifier {
    fn set_hostname_verification(&mut self, hostname: &str) -> Result<(), TlsError> {
        if hostname.is_empty() || ServerName::try_from(hostname).is_err() {
            return Err(TlsError::InvalidCertificate);
        }
        self.hostname = hostname.try_into().map_err(|_| TlsError::InsufficientSpace)?;
        Ok(())
    }

    fn verify_certificate(
        &mut self,
        transcript: &Sha256,
        certificate: CertificateRef,
    ) -> Result<(), TlsError> {
        self.leaf = None;
        self.transcript = None;
        let window = crate::tls::clock::validity_window()
            .ok_or(TlsError::InvalidCertificate)?;
        verify_chain(self.hostname.as_str(), window, crate::tls::roots::public_roots(), &certificate)?;
        let Some(CertificateEntryRef::X509(leaf)) = certificate.entries.first() else {
            return Err(TlsError::InvalidCertificate);
        };
        self.leaf = Some(Box::from(*leaf));
        self.transcript = Some(transcript.clone());
        Ok(())
    }

    fn verify_signature(&mut self, verify: CertificateVerifyRef) -> Result<(), TlsError> {
        let leaf = self.leaf.take().ok_or(TlsError::InvalidCertificate)?;
        let transcript = self.transcript.take().ok_or(TlsError::InvalidSignature)?;
        let der = CertificateDer::from(leaf.as_ref());
        let cert = webpki::EndEntityCert::try_from(&der)
            .map_err(|_| TlsError::InvalidCertificate)?;
        let algorithm = match verify.signature_scheme {
            SignatureScheme::EcdsaSecp256r1Sha256 => &Algorithm::P256Sha256,
            SignatureScheme::EcdsaSecp384r1Sha384 => &Algorithm::P384Sha384,
            SignatureScheme::RsaPssRsaeSha256 => &Algorithm::RsaPssSha256,
            SignatureScheme::RsaPssRsaeSha384 => &Algorithm::RsaPssSha384,
            SignatureScheme::RsaPssRsaeSha512 => &Algorithm::RsaPssSha512,
            SignatureScheme::Ed25519 => &Algorithm::Ed25519,
            // RSA PKCS#1 v1.5 is accepted on certificates, never for a TLS 1.3
            // CertificateVerify. No SHA-1 or legacy handshake signatures.
            _ => return Err(TlsError::InvalidSignatureScheme),
        };
        let mut message = [0x20; 130];
        let context = b"TLS 1.3, server CertificateVerify\0";
        message[64..98].copy_from_slice(context);
        message[98..].copy_from_slice(&transcript.finalize());
        cert.verify_signature(algorithm, &message, verify.signature)
            .map_err(|_| TlsError::InvalidSignature)
    }
}

/// The production certificate consumer and the offline regression tests share
/// this exact validation path. Only the caller selects the authenticated clock
/// window and trust store; there is no test clock or trust override in the guest.
fn verify_chain(
    hostname: &str,
    (lower, upper): (u64, u64),
    roots: &[pki_types::TrustAnchor<'_>],
    certificate: &CertificateRef<'_>,
) -> Result<(), TlsError> {
    if lower > upper {
        return Err(TlsError::InvalidCertificate);
    }
    let Some(CertificateEntryRef::X509(leaf)) = certificate.entries.first() else {
        return Err(TlsError::InvalidCertificate);
    };
    let leaf_der = CertificateDer::from(*leaf);
    let cert = webpki::EndEntityCert::try_from(&leaf_der)
        .map_err(|_| TlsError::InvalidCertificate)?;
    let mut intermediates = heapless::Vec::<CertificateDer<'_>, 16>::new();
    for entry in certificate.entries.iter().skip(1) {
        let CertificateEntryRef::X509(der) = entry else {
            return Err(TlsError::InvalidCertificate);
        };
        intermediates.push(CertificateDer::from(*der))
            .map_err(|_| TlsError::InsufficientSpace)?;
    }
    let path = cert.verify_for_usage(
        ALGORITHMS,
        roots,
        &intermediates,
        UnixTime::since_unix_epoch(Duration::from_secs(lower)),
        webpki::KeyUsage::server_auth(),
        None,
        None,
    ).map_err(|_| TlsError::InvalidCertificate)?;

    // Restrict the upper-end search to the lower-end path and anchor.
    // Thus a different chain valid only at the other endpoint cannot bridge
    // a validity gap. All certificates available at the upper endpoint have
    // already passed validity and constraints at the lower endpoint.
    let mut selected = heapless::Vec::<CertificateDer<'_>, 16>::new();
    for intermediate in path.intermediate_certificates() {
        selected.push(intermediate.der()).map_err(|_| TlsError::InsufficientSpace)?;
    }
    cert.verify_for_usage(
        ALGORITHMS,
        core::slice::from_ref(path.anchor()),
        &selected,
        UnixTime::since_unix_epoch(Duration::from_secs(upper)),
        webpki::KeyUsage::server_auth(),
        None,
        None,
    ).map_err(|_| TlsError::InvalidCertificate)?;
    let name = ServerName::try_from(hostname)
        .map_err(|_| TlsError::InvalidCertificate)?;
    cert.verify_is_valid_for_subject_name(&name)
        .map_err(|_| TlsError::InvalidCertificate)
}

#[derive(Debug)]
enum Algorithm {
    P256Sha256,
    P256Sha384,
    P384Sha256,
    P384Sha384,
    RsaPkcs1Sha256,
    RsaPkcs1Sha384,
    RsaPkcs1Sha512,
    RsaPssSha256,
    RsaPssSha384,
    RsaPssSha512,
    Ed25519,
}

static ALGORITHMS: &[&dyn SignatureVerificationAlgorithm] = &[
    &Algorithm::P256Sha256,
    &Algorithm::P256Sha384,
    &Algorithm::P384Sha256,
    &Algorithm::P384Sha384,
    &Algorithm::RsaPkcs1Sha256,
    &Algorithm::RsaPkcs1Sha384,
    &Algorithm::RsaPkcs1Sha512,
    &Algorithm::RsaPssSha256,
    &Algorithm::RsaPssSha384,
    &Algorithm::RsaPssSha512,
    &Algorithm::Ed25519,
];

impl SignatureVerificationAlgorithm for Algorithm {
    fn public_key_alg_id(&self) -> AlgorithmIdentifier {
        match self {
            Self::P256Sha256 | Self::P256Sha384 => alg_id::ECDSA_P256,
            Self::P384Sha256 | Self::P384Sha384 => alg_id::ECDSA_P384,
            Self::Ed25519 => alg_id::ED25519,
            _ => alg_id::RSA_ENCRYPTION,
        }
    }

    fn signature_alg_id(&self) -> AlgorithmIdentifier {
        match self {
            Self::P256Sha256 | Self::P384Sha256 => alg_id::ECDSA_SHA256,
            Self::P256Sha384 | Self::P384Sha384 => alg_id::ECDSA_SHA384,
            Self::RsaPkcs1Sha256 => alg_id::RSA_PKCS1_SHA256,
            Self::RsaPkcs1Sha384 => alg_id::RSA_PKCS1_SHA384,
            Self::RsaPkcs1Sha512 => alg_id::RSA_PKCS1_SHA512,
            Self::RsaPssSha256 => alg_id::RSA_PSS_SHA256,
            Self::RsaPssSha384 => alg_id::RSA_PSS_SHA384,
            Self::RsaPssSha512 => alg_id::RSA_PSS_SHA512,
            Self::Ed25519 => alg_id::ED25519,
        }
    }

    fn verify_signature(
        &self,
        public_key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), InvalidSignature> {
        match self {
            Self::P256Sha256 | Self::P256Sha384 => {
                let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(public_key)
                    .map_err(|_| InvalidSignature)?;
                let sig = p256::ecdsa::Signature::from_der(signature)
                    .map_err(|_| InvalidSignature)?;
                match self {
                    Self::P256Sha256 => key.verify_prehash(&Sha256::digest(message), &sig),
                    _ => key.verify_prehash(&Sha384::digest(message), &sig),
                }.map_err(|_| InvalidSignature)
            }
            Self::P384Sha256 | Self::P384Sha384 => {
                let key = p384::ecdsa::VerifyingKey::from_sec1_bytes(public_key)
                    .map_err(|_| InvalidSignature)?;
                let sig = p384::ecdsa::Signature::from_der(signature)
                    .map_err(|_| InvalidSignature)?;
                match self {
                    Self::P384Sha256 => key.verify_prehash(&Sha256::digest(message), &sig),
                    _ => key.verify_prehash(&Sha384::digest(message), &sig),
                }.map_err(|_| InvalidSignature)
            }
            Self::Ed25519 => {
                let key = ed25519_dalek::VerifyingKey::from_bytes(
                    public_key.try_into().map_err(|_| InvalidSignature)?,
                ).map_err(|_| InvalidSignature)?;
                let sig = ed25519_dalek::Signature::from_slice(signature)
                    .map_err(|_| InvalidSignature)?;
                key.verify_strict(message, &sig).map_err(|_| InvalidSignature)
            }
            _ => {
                let key = RsaPublicKey::from_pkcs1_der(public_key)
                    .map_err(|_| InvalidSignature)?;
                // Match modern WebPKI's RSA security floor and bound expensive
                // work on attacker-controlled keys before modular exponentiation.
                if !(2048..=RsaPublicKey::MAX_SIZE).contains(&key.n().bits()) {
                    return Err(InvalidSignature);
                }
                match self {
                    Self::RsaPkcs1Sha256 => key.verify(Pkcs1v15Sign::new::<Sha256>(), &Sha256::digest(message), signature),
                    Self::RsaPkcs1Sha384 => key.verify(Pkcs1v15Sign::new::<Sha384>(), &Sha384::digest(message), signature),
                    Self::RsaPkcs1Sha512 => key.verify(Pkcs1v15Sign::new::<Sha512>(), &Sha512::digest(message), signature),
                    Self::RsaPssSha256 => return verify_pss::<Sha256>(key, message, signature),
                    Self::RsaPssSha384 => return verify_pss::<Sha384>(key, message, signature),
                    Self::RsaPssSha512 => return verify_pss::<Sha512>(key, message, signature),
                    _ => return Err(InvalidSignature),
                }.map_err(|_| InvalidSignature)
            }
        }
    }
}

fn verify_pss<D: Digest + sha2::digest::FixedOutputReset>(
    key: RsaPublicKey,
    message: &[u8],
    signature: &[u8],
) -> Result<(), InvalidSignature> {
    use rsa::signature::Verifier;
    let signature = rsa::pss::Signature::try_from(signature).map_err(|_| InvalidSignature)?;
    rsa::pss::VerifyingKey::<D>::new(key)
        .verify(message, &signature)
        .map_err(|_| InvalidSignature)
}

#[cfg(test)]
#[path = "public_pki_test_fixtures.rs"]
mod fixtures;

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Signer;

    const VALID_TIME: u64 = 1_780_272_000; // 2026-06-01 UTC
    const VALID_WINDOW: (u64, u64) = (VALID_TIME, VALID_TIME + 60);
    const LEAF_NOT_BEFORE: u64 = 1_767_225_600; // 2026-01-01 UTC
    const LEAF_NOT_AFTER: u64 = 1_798_761_600; // 2027-01-01 UTC
    const INTERMEDIATE_NOT_BEFORE: u64 = 1_772_323_200; // 2026-03-01 UTC
    const INTERMEDIATE_NOT_AFTER: u64 = 1_788_220_800; // 2026-09-01 UTC

    fn fixture_chain<'a>(leaf: &'a [u8], intermediate: &'a [u8]) -> CertificateRef<'a> {
        let mut chain = CertificateRef::with_context(&[]);
        chain.add(CertificateEntryRef::X509(leaf)).unwrap();
        chain.add(CertificateEntryRef::X509(intermediate)).unwrap();
        chain
    }

    fn consume_fixture(
        hostname: &str,
        window: (u64, u64),
        leaf: &[u8],
        intermediate: &[u8],
    ) -> Result<(), TlsError> {
        let root = CertificateDer::from(fixtures::ROOT);
        let anchor = webpki::anchor_from_trusted_cert(&root).unwrap();
        verify_chain(hostname, window, &[anchor], &fixture_chain(leaf, intermediate))
    }

    #[test]
    fn certificate_consumer_accepts_valid_chain_and_single_label_wildcard() {
        for hostname in ["example.test", "www.example.test"] {
            assert!(consume_fixture(
                hostname, VALID_WINDOW, fixtures::LEAF, fixtures::INTERMEDIATE,
            ).is_ok());
        }
    }

    #[test]
    fn certificate_consumer_rejects_wrong_hostname_and_nested_wildcard() {
        for hostname in ["wrong.test", "example.test.attacker.test", "a.b.example.test"] {
            assert!(matches!(consume_fixture(
                hostname, VALID_WINDOW, fixtures::LEAF, fixtures::INTERMEDIATE,
            ), Err(TlsError::InvalidCertificate)));
        }
    }

    #[test]
    fn certificate_consumer_rejects_leaf_outside_either_time_bound() {
        // This intermediate spans 2025..2030, so it cannot mask a missing
        // leaf-date check at either endpoint.
        assert!(consume_fixture(
            "example.test", VALID_WINDOW, fixtures::LEAF, fixtures::WIDE_INTERMEDIATE,
        ).is_ok());
        for window in [
            (LEAF_NOT_BEFORE - 1, VALID_TIME),
            (VALID_TIME, LEAF_NOT_AFTER + 1),
        ] {
            assert!(matches!(consume_fixture(
                "example.test", window, fixtures::LEAF, fixtures::WIDE_INTERMEDIATE,
            ), Err(TlsError::InvalidCertificate)));
        }
    }

    #[test]
    fn certificate_consumer_checks_intermediate_dates_at_both_bounds() {
        // The leaf remains valid at every endpoint in these windows. Failure
        // therefore requires checking the intermediate, not just the leaf.
        for window in [
            (INTERMEDIATE_NOT_BEFORE - 1, VALID_TIME),
            (VALID_TIME, INTERMEDIATE_NOT_AFTER + 1),
        ] {
            assert!(window.0 >= LEAF_NOT_BEFORE && window.1 <= LEAF_NOT_AFTER);
            assert!(matches!(consume_fixture(
                "example.test", window, fixtures::LEAF, fixtures::INTERMEDIATE,
            ), Err(TlsError::InvalidCertificate)));
        }
    }

    #[test]
    fn certificate_consumer_rejects_tampered_leaf_and_intermediate_signatures() {
        let mut leaf = fixtures::LEAF.to_vec();
        let mut intermediate = fixtures::INTERMEDIATE.to_vec();
        *leaf.last_mut().unwrap() ^= 1;
        *intermediate.last_mut().unwrap() ^= 1;
        for (leaf, intermediate) in [
            (leaf.as_slice(), fixtures::INTERMEDIATE),
            (fixtures::LEAF, intermediate.as_slice()),
        ] {
            assert!(matches!(consume_fixture(
                "example.test", VALID_WINDOW, leaf, intermediate,
            ), Err(TlsError::InvalidCertificate)));
        }
    }

    #[test]
    fn certificate_consumer_requires_trusted_anchor_and_complete_chain() {
        let chain = fixture_chain(fixtures::LEAF, fixtures::INTERMEDIATE);
        assert!(matches!(
            verify_chain("example.test", VALID_WINDOW, &[], &chain),
            Err(TlsError::InvalidCertificate),
        ));
        assert!(matches!(consume_fixture(
            "example.test", (VALID_TIME + 60, VALID_TIME), fixtures::LEAF, fixtures::INTERMEDIATE,
        ), Err(TlsError::InvalidCertificate)));
        let root = CertificateDer::from(fixtures::ROOT);
        let anchor = webpki::anchor_from_trusted_cert(&root).unwrap();
        let mut missing_intermediate = CertificateRef::with_context(&[]);
        missing_intermediate.add(CertificateEntryRef::X509(fixtures::LEAF)).unwrap();
        assert!(matches!(
            verify_chain("example.test", VALID_WINDOW, &[anchor], &missing_intermediate),
            Err(TlsError::InvalidCertificate),
        ));
    }

    #[test]
    fn hostname_is_required_and_must_be_a_valid_server_name() {
        for hostname in ["", "*.example.com", "example.com/path", "bad host"] {
            assert!(PublicVerifier::new(hostname).is_err());
        }
        for hostname in ["example.com", "tuoitre.vn", "sub.example.com"] {
            assert!(PublicVerifier::new(hostname).is_ok());
        }
    }

    #[test]
    fn signature_requires_a_previously_validated_certificate() {
        let mut verifier = PublicVerifier::new("example.com").unwrap();
        let verify = CertificateVerifyRef {
            signature_scheme: SignatureScheme::EcdsaSecp256r1Sha256,
            signature: &[],
        };
        assert!(verifier.verify_signature(verify).is_err());
    }

    #[test]
    fn rustcrypto_p256_adapter_authenticates_message_and_key() {
        let key = p256::ecdsa::SigningKey::from_slice(&[1; 32]).unwrap();
        let signature: p256::ecdsa::Signature = key.sign(b"certificate transcript");
        let der = signature.to_der();
        let public = key.verifying_key().to_encoded_point(false);
        assert!(Algorithm::P256Sha256.verify_signature(
            public.as_bytes(), b"certificate transcript", der.as_bytes(),
        ).is_ok());
        assert!(Algorithm::P256Sha256.verify_signature(
            public.as_bytes(), b"changed transcript", der.as_bytes(),
        ).is_err());
        assert!(Algorithm::P256Sha256.verify_signature(
            &[0; 65], b"certificate transcript", der.as_bytes(),
        ).is_err());
    }

    #[test]
    fn rustcrypto_p384_adapter_authenticates_message() {
        let key = p384::ecdsa::SigningKey::from_slice(&[1; 48]).unwrap();
        let signature: p384::ecdsa::Signature = key.sign(b"certificate transcript");
        let der = signature.to_der();
        let public = key.verifying_key().to_encoded_point(false);
        assert!(Algorithm::P384Sha384.verify_signature(
            public.as_bytes(), b"certificate transcript", der.as_bytes(),
        ).is_ok());
        assert!(Algorithm::P384Sha384.verify_signature(
            public.as_bytes(), b"changed transcript", der.as_bytes(),
        ).is_err());
    }
}
