#![no_std]
#![forbid(unsafe_code)]

//! Allocation-free, closed-policy validation for authenticated enrollment profiles.
//! Profiles are strict DER certificates concatenated leaf first; trust roots are policy only.

mod adapter;
mod admission;
mod cert;
mod der;
mod error;
mod extensions;
mod metadata;
mod path;
mod pending;
mod policy;
mod time;
mod tpm;
mod transaction;
#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;

use cert::Certificate;
use sha2::{Digest, Sha256};

pub use error::Error;
pub use metadata::ValidatedProfileMetadata;
pub use pending::{PendingPublicReader, PendingPublicRequest, PublicReadError, MAX_TPM2B_PUBLIC};
pub use policy::{DeniedSerial, TrustedPolicy};
pub use stm32_authority_journal::PendingEnrollmentSnapshot;
pub use transaction::{validate_and_stage_profile, PendingSnapshotSource, ProfileStageError};

/// Maximum accepted raw leaf-first certificate profile size.
pub const MAX_PROFILE_LEN: usize = 12_288;
/// Maximum accepted certificate count, excluding the trust anchor.
pub const MAX_CERTIFICATES: usize = 3;

/// Validates one admitted uploaded profile against trusted policy and protected TPM state.
///
/// `admitted` is the opaque authority-protocol capability created only after authenticated
/// state admission persisted the request's boot, identity, and sequence floor. `profile`
/// must be one to three complete strict-DER X.509 certificates concatenated leaf first,
/// without a root or framing bytes. `policy` supplies the DER trust anchor, exact DNS
/// identity, trusted signed time, exact journal revision, current slot/generation, and denylists.
/// `pending_snapshot` must be issued by journal recovery plus profile-bank authentication and
/// exactly mirror the admitted request's domain, CSR, profile, and TPM bindings. `public_reader` is
/// invoked twice and must return the exact canonical length-prefixed `TPM2B_PUBLIC` for its
/// typed request. The function performs no network access and does not use AIA, OCSP, or CN.
///
/// On success, returns non-constructible [`ValidatedProfileMetadata`] borrowing only the
/// leaf serial from `profile`. Any admission-binding, framing, algorithm, path, identity,
/// freshness, digest, or double-read failure returns a specific [`Error`] and no validated
/// value. No pre-admission profile-validation entry point is exposed.
pub fn validate_profile<'a, R: PendingPublicReader>(
    admitted: &authority_protocol::AdmittedProfileValidation,
    profile: &'a [u8],
    policy: TrustedPolicy<'_>,
    pending_snapshot: &PendingEnrollmentSnapshot,
    public_reader: &mut R,
) -> Result<ValidatedProfileMetadata<'a>, Error> {
    let request = admitted.request();
    let binding = admission::AdmissionBinding::from_admitted(admitted);
    if !admission::matches(pending_snapshot, policy, binding) {
        return Err(Error::StaleSnapshot);
    }
    if pending_snapshot.profile_digest() != &request.profile_digest {
        return Err(Error::ProfileDigestMismatch);
    }
    if pending_snapshot.spki_digest() != &request.pending_spki_digest {
        return Err(Error::SpkiMismatch);
    }
    if pending_snapshot.tpm_public_digest() != &request.tpm_public_digest {
        return Err(Error::TpmPublicDigestMismatch);
    }
    validate_profile_core(profile, policy, pending_snapshot, public_reader)
}

fn validate_profile_core<'a, R: PendingPublicReader>(
    profile: &'a [u8],
    policy: TrustedPolicy<'_>,
    pending_snapshot: &PendingEnrollmentSnapshot,
    public_reader: &mut R,
) -> Result<ValidatedProfileMetadata<'a>, Error> {
    if profile.is_empty() || profile.len() > MAX_PROFILE_LEN {
        return Err(Error::ProfileSize);
    }
    if pending_snapshot.journal_revision() != policy.expected_journal_revision
        || pending_snapshot.protected_revision() != policy.expected_journal_revision
        || pending_snapshot.pending_slot() != policy.expected_slot
        || pending_snapshot.generation() != policy.expected_generation
        || pending_snapshot.policy_epoch() != policy.expected_policy_epoch
        || pending_snapshot.upload_handle() == 0
    {
        return Err(Error::StaleSnapshot);
    }
    if pending_snapshot.profile_len() as usize != profile.len() {
        return Err(Error::ProfileDigestMismatch);
    }
    let profile_digest: [u8; 32] = Sha256::digest(profile).into();
    if profile_digest != *pending_snapshot.profile_digest() {
        return Err(Error::ProfileDigestMismatch);
    }
    let mut stream = der::Reader::new(profile);
    let mut certificates = [None; MAX_CERTIFICATES];
    let mut count = 0;
    while !stream.is_empty() {
        if count == MAX_CERTIFICATES {
            return Err(Error::ProfileSize);
        }
        let encoded = stream.required(0x30)?;
        certificates[count] = Some(Certificate::parse(encoded.full)?);
        count += 1;
    }
    let anchor = Certificate::parse(policy.trust_anchor_der)?;
    let leaf = certificates[0].ok_or(Error::ProfileSize)?;
    let (node_id, spki_digest) = match count {
        1 => path::validate(&[leaf], anchor, policy)?,
        2 => path::validate(
            &[leaf, certificates[1].ok_or(Error::ProfileSize)?],
            anchor,
            policy,
        )?,
        3 => path::validate(
            &[
                leaf,
                certificates[1].ok_or(Error::ProfileSize)?,
                certificates[2].ok_or(Error::ProfileSize)?,
            ],
            anchor,
            policy,
        )?,
        _ => return Err(Error::ProfileSize),
    };
    if spki_digest != *pending_snapshot.spki_digest() {
        return Err(Error::SpkiMismatch);
    }
    let tpm_spki = pending::verify(pending_snapshot, public_reader)?;
    if leaf.spki != tpm_spki.as_slice() {
        return Err(Error::SpkiMismatch);
    }
    Ok(ValidatedProfileMetadata {
        slot: pending_snapshot.pending_slot(),
        generation: pending_snapshot.generation(),
        profile_len: pending_snapshot.profile_len(),
        profile_digest,
        spki_digest,
        node_id,
        serial: leaf.serial,
        tpm_public_digest: *pending_snapshot.tpm_public_digest(),
    })
}
