use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::{cert::Certificate, policy::TrustedPolicy, Error};

pub(crate) fn validate(
    certificates: &[Certificate<'_>],
    anchor: Certificate<'_>,
    policy: TrustedPolicy<'_>,
) -> Result<([u8; 32], [u8; 32]), Error> {
    require_valid_public_key(anchor)?;
    let anchor_digest: [u8; 32] = Sha256::digest(anchor.full).into();
    let mut digests = [[0u8; 32]; 3];
    for (index, certificate) in certificates.iter().enumerate() {
        let digest: [u8; 32] = Sha256::digest(certificate.full).into();
        if digest == anchor_digest
            || digests[..index].contains(&digest)
            || certificate.spki == anchor.spki
            || certificates[..index]
                .iter()
                .any(|prior| prior.spki == certificate.spki)
        {
            return Err(Error::ForbiddenCertificate);
        }
        digests[index] = digest;
    }
    check_ca(anchor, certificates.len() - 1)?;
    check_time(anchor, policy.signed_time_unix)?;
    check_name_constraints(anchor, policy.expected_dns_name)?;
    for (index, certificate) in certificates.iter().enumerate() {
        require_valid_public_key(*certificate)?;
        check_time(*certificate, policy.signed_time_unix)?;
        if index == 0 {
            check_leaf(*certificate, policy)?;
        } else {
            check_ca(*certificate, index - 1)?;
            check_name_constraints(*certificate, policy.expected_dns_name)?;
        }
        let parent = certificates.get(index + 1).copied().unwrap_or(anchor);
        check_link(*certificate, parent)?;
        verify_signature(*certificate, parent)?;
    }
    let leaf = certificates[0];
    let spki_digest: [u8; 32] = Sha256::digest(leaf.spki).into();
    let node_id: [u8; 32] = leaf
        .extensions
        .node_id
        .ok_or(Error::InvalidNodeId)?
        .try_into()
        .map_err(|_| Error::InvalidNodeId)?;
    Ok((node_id, spki_digest))
}

fn require_valid_public_key(certificate: Certificate<'_>) -> Result<(), Error> {
    VerifyingKey::from_sec1_bytes(certificate.public_key)
        .map(|_| ())
        .map_err(|_| Error::UnsupportedPublicKey)
}

fn check_leaf(certificate: Certificate<'_>, policy: TrustedPolicy<'_>) -> Result<(), Error> {
    if certificate
        .extensions
        .ca
        .is_some_and(|(ca, path)| ca || path.is_some())
    {
        return Err(Error::InvalidBasicConstraints);
    }
    if certificate
        .extensions
        .permitted_dns
        .iter()
        .any(Option::is_some)
        || certificate
            .extensions
            .excluded_dns
            .iter()
            .any(Option::is_some)
    {
        return Err(Error::InvalidNameConstraints);
    }
    if certificate.extensions.key_usage != Some(0x8000) {
        return Err(Error::InvalidKeyUsage);
    }
    if certificate.extensions.eku_client_only != Some(true) {
        return Err(Error::InvalidExtendedKeyUsage);
    }
    let (san, san_critical) = certificate.extensions.san.ok_or(Error::InvalidSan)?;
    if san != policy.expected_dns_name || (certificate.subject_empty && !san_critical) {
        return Err(Error::InvalidSan);
    }
    let spki_digest: [u8; 32] = Sha256::digest(certificate.spki).into();
    if certificate.extensions.node_id != Some(spki_digest.as_slice()) {
        return Err(Error::InvalidNodeId);
    }
    if policy.denied_node_ids.contains(&spki_digest)
        || policy
            .denied_serials
            .iter()
            .any(|serial| serial.as_bytes() == certificate.serial)
    {
        return Err(Error::Denied);
    }
    Ok(())
}

fn check_ca(certificate: Certificate<'_>, subordinate_cas: usize) -> Result<(), Error> {
    let (ca, path_len) = certificate
        .extensions
        .ca
        .ok_or(Error::InvalidBasicConstraints)?;
    if !ca {
        return Err(Error::InvalidBasicConstraints);
    }
    let usage = certificate
        .extensions
        .key_usage
        .ok_or(Error::InvalidKeyUsage)?;
    if usage & 0x0400 == 0 || (usage & 0x0180 != 0 && usage & 0x0800 == 0) {
        return Err(Error::InvalidKeyUsage);
    }
    if certificate.extensions.eku_client_only == Some(false) {
        return Err(Error::InvalidExtendedKeyUsage);
    }
    if path_len.is_some_and(|limit| subordinate_cas > limit as usize) {
        return Err(Error::PathLength);
    }
    if certificate.extensions.ski.is_none() {
        return Err(Error::InvalidSubjectKeyIdentifier);
    }
    Ok(())
}

fn check_link(child: Certificate<'_>, parent: Certificate<'_>) -> Result<(), Error> {
    let aki = child
        .extensions
        .aki
        .ok_or(Error::InvalidAuthorityKeyIdentifier)?;
    let ski = parent
        .extensions
        .ski
        .ok_or(Error::InvalidSubjectKeyIdentifier)?;
    if child.issuer != parent.subject || aki != ski {
        return Err(Error::ChainLink);
    }
    Ok(())
}

fn verify_signature(child: Certificate<'_>, parent: Certificate<'_>) -> Result<(), Error> {
    let key = VerifyingKey::from_sec1_bytes(parent.public_key)
        .map_err(|_| Error::UnsupportedPublicKey)?;
    let signature =
        Signature::from_der(child.signature).map_err(|_| Error::InvalidSignatureEncoding)?;
    key.verify(child.tbs, &signature)
        .map_err(|_| Error::SignatureVerification)
}

fn check_time(certificate: Certificate<'_>, now: i64) -> Result<(), Error> {
    if now < certificate.not_before || now > certificate.not_after {
        Err(Error::CertificateExpired)
    } else {
        Ok(())
    }
}

fn check_name_constraints(certificate: Certificate<'_>, dns: &[u8]) -> Result<(), Error> {
    let permitted = certificate.extensions.permitted_dns;
    let excluded = certificate.extensions.excluded_dns;
    if excluded
        .iter()
        .flatten()
        .any(|constraint| dns_within(dns, constraint))
    {
        return Err(Error::InvalidNameConstraints);
    }
    if permitted.iter().any(Option::is_some)
        && !permitted
            .iter()
            .flatten()
            .any(|constraint| dns_within(dns, constraint))
    {
        return Err(Error::InvalidNameConstraints);
    }
    Ok(())
}

fn dns_within(name: &[u8], constraint: &[u8]) -> bool {
    let (constraint, require_subdomain) = constraint
        .strip_prefix(b".")
        .map_or((constraint, false), |suffix| (suffix, true));
    if name.eq_ignore_ascii_case(constraint) {
        return !require_subdomain;
    }
    name.len() > constraint.len()
        && name[name.len() - constraint.len()..].eq_ignore_ascii_case(constraint)
        && name[name.len() - constraint.len() - 1] == b'.'
}
