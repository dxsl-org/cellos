//! Strict DER framing and identity binding for the mounted client chain.
//!
//! Bounds are frozen: at most [`RELAY_CHAIN_MAX_CERTS`] certificates and
//! [`RELAY_CHAIN_MAX_LEN`] bytes. The leaf must carry the clientAuth EKU
//! and must not carry the serverAuth EKU; the served SPKI from KMS
//! (opcode 14) must hash to the deployment NodeId.

use sha2::{Digest, Sha256};
use types::kms::{RELAY_CHAIN_MAX_CERTS, RELAY_CHAIN_MAX_LEN};

/// id-ce-extKeyUsage extension OID content (2.5.29.37).
const EXTENDED_KEY_USAGE_OID: [u8; 3] = [0x55, 0x1D, 0x25];
/// clientAuth extended-key-usage OID content (1.3.6.1.5.5.7.3.2).
const CLIENT_AUTH_EKU: [u8; 8] = [0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x02];
/// serverAuth extended-key-usage OID content (1.3.6.1.5.5.7.3.1).
const SERVER_AUTH_EKU: [u8; 8] = [0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01];

#[derive(Debug, PartialEq, Eq)]
pub enum ChainError {
    /// Too many certificates for the frozen bound.
    TooManyCertificates,
    /// Chain exceeds the frozen byte budget.
    TooLong,
    /// A certificate is not well-framed DER SEQUENCE data.
    MalformedDer,
    /// The same certificate appears twice in the chain.
    DuplicateCertificate,
    /// Leaf lacks clientAuth or carries serverAuth.
    LeafUsageNotAllowed,
    /// Leaf SPKI or NodeId facts disagree.
    IdentityMismatch,
}

/// Read one DER TLV header; returns `(tag, content_len, header_len)`.
fn read_tlv(bytes: &[u8]) -> Option<(u8, usize, usize)> {
    let (&tag, rest) = bytes.split_first()?;
    let &first = rest.first()?;
    if first < 0x80 {
        return Some((tag, first as usize, 2));
    }
    let count = (first & 0x7F) as usize;
    if count == 0 || count > 4 || rest.len() < 1 + count {
        return None;
    }
    if rest[1] == 0 {
        return None;
    }
    let mut len = 0usize;
    for &byte in &rest[1..1 + count] {
        len = (len << 8) | byte as usize;
    }
    // Canonical DER forbids non-minimal long-form lengths.
    (len >= 0x80).then_some((tag, len, 2 + count))
}
/// Consume exactly one DER TLV with `expected_tag`.
///
/// The returned pair is `(full_tlv, content)`. All slices borrow the bounded
/// input chain; parsing never allocates.
fn take_tlv<'a>(
    bytes: &mut &'a [u8],
    expected_tag: u8,
) -> Result<(&'a [u8], &'a [u8]), ChainError> {
    let input = *bytes;
    let (tag, len, head) = read_tlv(input).ok_or(ChainError::MalformedDer)?;
    let total = head.checked_add(len).ok_or(ChainError::MalformedDer)?;
    if tag != expected_tag || total > input.len() {
        return Err(ChainError::MalformedDer);
    }
    let (full, rest) = input.split_at(total);
    *bytes = rest;
    Ok((full, &full[head..]))
}

#[derive(Clone, Copy)]
struct LeafProfile<'a> {
    spki: &'a [u8],
}

fn validate_eku(encoded: &[u8]) -> Result<(), ChainError> {
    let mut outer = encoded;
    let (_, mut oids) = take_tlv(&mut outer, 0x30)?;
    if !outer.is_empty() {
        return Err(ChainError::MalformedDer);
    }
    let mut client_auth = false;
    let mut server_auth = false;
    while !oids.is_empty() {
        let (_, oid) = take_tlv(&mut oids, 0x06)?;
        client_auth |= oid == CLIENT_AUTH_EKU;
        server_auth |= oid == SERVER_AUTH_EKU;
    }
    if !client_auth || server_auth {
        return Err(ChainError::LeafUsageNotAllowed);
    }
    Ok(())
}

fn validate_extensions(encoded: &[u8]) -> Result<(), ChainError> {
    let mut explicit = encoded;
    let (_, mut extensions) = take_tlv(&mut explicit, 0x30)?;
    if !explicit.is_empty() {
        return Err(ChainError::MalformedDer);
    }
    let mut found_eku = false;
    while !extensions.is_empty() {
        let (_, mut extension) = take_tlv(&mut extensions, 0x30)?;
        let (_, oid) = take_tlv(&mut extension, 0x06)?;
        if extension.first() == Some(&0x01) {
            let (_, critical) = take_tlv(&mut extension, 0x01)?;
            if critical.len() != 1 || !matches!(critical[0], 0x00 | 0xFF) {
                return Err(ChainError::MalformedDer);
            }
        }
        let (_, value) = take_tlv(&mut extension, 0x04)?;
        if !extension.is_empty() {
            return Err(ChainError::MalformedDer);
        }
        if oid == EXTENDED_KEY_USAGE_OID {
            if found_eku {
                return Err(ChainError::MalformedDer);
            }
            validate_eku(value)?;
            found_eku = true;
        }
    }
    if !found_eku {
        return Err(ChainError::LeafUsageNotAllowed);
    }
    Ok(())
}

/// Walk the X.509 Certificate/TBSCertificate structure and return the canonical
/// leaf SPKI slice after enforcing the client-only EKU profile.
fn leaf_profile(leaf: &[u8]) -> Result<LeafProfile<'_>, ChainError> {
    let mut outer = leaf;
    let (_, mut certificate) = take_tlv(&mut outer, 0x30)?;
    if !outer.is_empty() {
        return Err(ChainError::MalformedDer);
    }
    let (_, mut tbs) = take_tlv(&mut certificate, 0x30)?;
    take_tlv(&mut certificate, 0x30)?;
    take_tlv(&mut certificate, 0x03)?;
    if !certificate.is_empty() {
        return Err(ChainError::MalformedDer);
    }

    // Relay leaves are X.509 v3. The explicit version is INTEGER 2.
    let (_, version) = take_tlv(&mut tbs, 0xA0)?;
    let mut version = version;
    let (_, version_number) = take_tlv(&mut version, 0x02)?;
    if !version.is_empty() || version_number != [0x02] {
        return Err(ChainError::MalformedDer);
    }
    take_tlv(&mut tbs, 0x02)?; // serialNumber
    take_tlv(&mut tbs, 0x30)?; // signature
    take_tlv(&mut tbs, 0x30)?; // issuer
    take_tlv(&mut tbs, 0x30)?; // validity
    take_tlv(&mut tbs, 0x30)?; // subject
    let (spki, _) = take_tlv(&mut tbs, 0x30)?;

    let mut extensions_seen = false;
    let mut issuer_uid_seen = false;
    let mut subject_uid_seen = false;
    while let Some(tag) = tbs.first().copied() {
        match tag {
            0x81 if !issuer_uid_seen && !extensions_seen => {
                take_tlv(&mut tbs, 0x81)?;
                issuer_uid_seen = true;
            }
            0x82 if !subject_uid_seen && !extensions_seen => {
                take_tlv(&mut tbs, 0x82)?;
                subject_uid_seen = true;
            }
            0xA3 if !extensions_seen => {
                let (_, extensions) = take_tlv(&mut tbs, 0xA3)?;
                validate_extensions(extensions)?;
                extensions_seen = true;
            }
            _ => return Err(ChainError::MalformedDer),
        }
    }
    if !extensions_seen {
        return Err(ChainError::LeafUsageNotAllowed);
    }
    Ok(LeafProfile { spki })
}

/// Frame-check a concatenated DER chain and enforce the frozen bounds.
///
/// Returns the per-certificate DER slices. Duplicate certificates (exact
/// byte equality) are rejected so a stale re-issued leaf cannot shadow a
/// rotation.
pub fn frame_chain(
    chain: &[u8],
) -> Result<heapless::Vec<&[u8], RELAY_CHAIN_MAX_CERTS>, ChainError> {
    if chain.len() > RELAY_CHAIN_MAX_LEN {
        return Err(ChainError::TooLong);
    }
    let mut certs = heapless::Vec::<&[u8], RELAY_CHAIN_MAX_CERTS>::new();
    let mut rest = chain;
    while !rest.is_empty() {
        if certs.len() == RELAY_CHAIN_MAX_CERTS {
            return Err(ChainError::TooManyCertificates);
        }
        let (tag, len, head) = read_tlv(rest).ok_or(ChainError::MalformedDer)?;
        let total = head.checked_add(len).ok_or(ChainError::MalformedDer)?;
        if tag != 0x30 || total > rest.len() {
            return Err(ChainError::MalformedDer);
        }
        let (cert, tail) = rest.split_at(total);
        certs
            .push(cert)
            .map_err(|_| ChainError::TooManyCertificates)?;
        rest = tail;
    }
    for index in 0..certs.len() {
        for later in &certs[index + 1..] {
            if certs[index] == *later {
                return Err(ChainError::DuplicateCertificate);
            }
        }
    }
    Ok(certs)
}

/// Validate leaf usage: clientAuth must occur inside EKU, and serverAuth must
/// not occur there. OID-shaped bytes in other certificate fields/extensions
/// have no effect.
pub fn validate_leaf_usage(leaf: &[u8]) -> Result<(), ChainError> {
    leaf_profile(leaf).map(|_| ())
}

/// Full acceptance pipeline for one mounted chain.
pub fn validate_chain(chain: &[u8]) -> Result<(), ChainError> {
    let certs = frame_chain(chain)?;
    match certs.split_first() {
        Some((leaf, _)) => validate_leaf_usage(leaf),
        None => Err(ChainError::MalformedDer),
    }
}

/// Validate a mounted active chain against opcode 14 and both active NodeId
/// metadata sources.
///
/// This function must not authorize opcode 13 staging: the frozen opcode-14
/// response exposes only the active generation, not an enrollment's pending
/// key. Initial enrollment and renewal therefore remain fail-closed until the
/// supervisor can supply an authenticated pending-key binding without changing
/// the provisioning ABI.
pub fn validate_active_chain(
    chain: &[u8],
    active_spki_sec1: &[u8; 65],
    active_spki_sha256: &[u8; 32],
    kms_metadata_node_id: &[u8; 32],
    manifest_node_id: &[u8; 32],
) -> Result<(), ChainError> {
    let certs = frame_chain(chain)?;
    let leaf = certs.first().ok_or(ChainError::MalformedDer)?;
    let profile = leaf_profile(leaf)?;
    let (canonical_spki, canonical_len) =
        types::kms::p256_spki_der(active_spki_sec1).ok_or(ChainError::IdentityMismatch)?;
    if profile.spki != &canonical_spki[..canonical_len] {
        return Err(ChainError::IdentityMismatch);
    }
    let leaf_digest: [u8; 32] = Sha256::digest(profile.spki).into();
    if leaf_digest != *active_spki_sha256
        || leaf_digest != *kms_metadata_node_id
        || leaf_digest != *manifest_node_id
    {
        return Err(ChainError::IdentityMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::*;
    use alloc::vec::Vec;

    fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(tag);
        if content.len() < 0x80 {
            out.push(content.len() as u8);
        } else {
            assert!(content.len() <= u8::MAX as usize);
            out.extend_from_slice(&[0x81, content.len() as u8]);
        }
        out.extend_from_slice(content);
        out
    }

    fn append(target: &mut Vec<u8>, encoded: Vec<u8>) {
        target.extend_from_slice(&encoded);
    }

    fn extension(oid: &[u8], value: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        append(&mut body, tlv(0x06, oid));
        append(&mut body, tlv(0x04, value));
        tlv(0x30, &body)
    }

    fn relay_leaf(
        spki_sec1: &[u8; 65],
        eku_oids: Option<&[&[u8]]>,
        unrelated_value: Option<&[u8]>,
    ) -> Vec<u8> {
        let (spki, spki_len) = types::kms::p256_spki_der(spki_sec1).unwrap();
        let mut extensions = Vec::new();
        if let Some(value) = unrelated_value {
            append(&mut extensions, extension(&[0x2A, 0x03], value));
        }
        if let Some(oids) = eku_oids {
            let mut usages = Vec::new();
            for oid in oids {
                append(&mut usages, tlv(0x06, oid));
            }
            append(
                &mut extensions,
                extension(&EXTENDED_KEY_USAGE_OID, &tlv(0x30, &usages)),
            );
        }

        let mut tbs = Vec::new();
        append(&mut tbs, tlv(0xA0, &tlv(0x02, &[0x02])));
        append(&mut tbs, tlv(0x02, &[0x01]));
        append(&mut tbs, tlv(0x30, &[]));
        append(&mut tbs, tlv(0x30, &[]));
        append(&mut tbs, tlv(0x30, &[]));
        append(&mut tbs, tlv(0x30, &[]));
        tbs.extend_from_slice(&spki[..spki_len]);
        append(&mut tbs, tlv(0xA3, &tlv(0x30, &extensions)));

        let mut certificate = Vec::new();
        append(&mut certificate, tlv(0x30, &tbs));
        append(&mut certificate, tlv(0x30, &[]));
        append(&mut certificate, tlv(0x03, &[0x00]));
        tlv(0x30, &certificate)
    }

    fn framed_cert(body_len: u8) -> heapless::Vec<u8, 512> {
        let mut out = heapless::Vec::new();
        out.push(0x30).unwrap();
        out.push(body_len).unwrap();
        out.extend(0xA5..0xA5 + body_len);
        out
    }

    #[test]
    fn bounds_and_duplicates_are_enforced() {
        let single = framed_cert(4);
        assert!(frame_chain(&single).is_ok());
        let mut duplicate: heapless::Vec<u8, 512> = single.clone();
        duplicate.extend(single.iter().copied());
        assert_eq!(
            frame_chain(&duplicate),
            Err(ChainError::DuplicateCertificate)
        );
        let mut four: heapless::Vec<u8, 512> = heapless::Vec::new();
        for _ in 0..RELAY_CHAIN_MAX_CERTS + 1 {
            four.extend(framed_cert(4).iter().copied());
        }
        assert_eq!(frame_chain(&four), Err(ChainError::TooManyCertificates));
        assert_eq!(frame_chain(&[0x30, 0x82]), Err(ChainError::MalformedDer));
        let big = [0u8; RELAY_CHAIN_MAX_LEN + 1];
        assert_eq!(frame_chain(&big), Err(ChainError::TooLong));
    }

    #[test]
    fn oid_bytes_outside_eku_neither_authorize_nor_deny_usage() {
        let mut key = [0x11; 65];
        key[0] = 0x04;
        let client_oid = tlv(0x06, &CLIENT_AUTH_EKU);
        let missing_eku = relay_leaf(&key, None, Some(&client_oid));
        assert_eq!(
            validate_leaf_usage(&missing_eku),
            Err(ChainError::LeafUsageNotAllowed)
        );

        let server_oid = tlv(0x06, &SERVER_AUTH_EKU);
        let allowed = relay_leaf(&key, Some(&[&CLIENT_AUTH_EKU]), Some(&server_oid));
        assert!(validate_leaf_usage(&allowed).is_ok());
        let denied = relay_leaf(&key, Some(&[&CLIENT_AUTH_EKU, &SERVER_AUTH_EKU]), None);
        assert_eq!(
            validate_leaf_usage(&denied),
            Err(ChainError::LeafUsageNotAllowed)
        );
    }

    #[test]
    fn active_chain_binds_leaf_key_and_all_node_id_sources() {
        let mut kms_key = [0x11; 65];
        kms_key[0] = 0x04;
        let leaf = relay_leaf(&kms_key, Some(&[&CLIENT_AUTH_EKU]), None);
        let (spki, len) = types::kms::p256_spki_der(&kms_key).unwrap();
        let node_id: [u8; 32] = Sha256::digest(&spki[..len]).into();
        assert!(validate_active_chain(&leaf, &kms_key, &node_id, &node_id, &node_id).is_ok());

        let mut wrong_leaf_key = kms_key;
        wrong_leaf_key[1] ^= 0xFF;
        let wrong_leaf = relay_leaf(&wrong_leaf_key, Some(&[&CLIENT_AUTH_EKU]), None);
        assert_eq!(
            validate_active_chain(&wrong_leaf, &kms_key, &node_id, &node_id, &node_id),
            Err(ChainError::IdentityMismatch)
        );
        let mut wrong_metadata = node_id;
        wrong_metadata[0] ^= 0xFF;
        assert_eq!(
            validate_active_chain(&leaf, &kms_key, &node_id, &wrong_metadata, &node_id),
            Err(ChainError::IdentityMismatch)
        );
        assert_eq!(validate_chain(&[]), Err(ChainError::MalformedDer));
    }
}
