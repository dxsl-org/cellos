//! Non-forgeable operation-specific validation tokens.

use crate::{
    verify_typed_request, AcceptSignedTimeRequest, AuthorityFault, FrameHeader,
    RequestAuthenticator, ValidateAndStageRelayProfileRequest, ValidatedRequest, DIGEST_LEN,
    SIGNATURE_LEN,
};

pub trait SignedTimeVerifier {
    fn verify_signed_time(&self, request: &AcceptSignedTimeRequest) -> bool;
}

pub trait RootProfileVerifier {
    fn verify_root_profile(&self, admitted: &AdmittedProfileValidation) -> bool;
}

pub trait BootMeasurementVerifier {
    fn verify_boot_measurement(&self, loader_digest: &[u8; DIGEST_LEN]) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedBootMeasurement([u8; DIGEST_LEN]);

impl VerifiedBootMeasurement {
    pub(crate) const fn loader_digest(&self) -> &[u8; DIGEST_LEN] {
        &self.0
    }
}

pub fn verify_boot_measurement<V: BootMeasurementVerifier>(
    loader_digest: [u8; DIGEST_LEN],
    verifier: &V,
) -> Result<VerifiedBootMeasurement, AuthorityFault> {
    if !verifier.verify_boot_measurement(&loader_digest) {
        return Err(AuthorityFault::ProfileRejected);
    }
    Ok(VerifiedBootMeasurement(loader_digest))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCasReceipt {
    pub device_id: [u8; crate::ID_LEN],
    pub authority_id: [u8; crate::ID_LEN],
    pub authority_epoch: u64,
    pub generation: u64,
    pub policy_epoch: u64,
    pub pending_slot: u8,
    pub pending_spki_digest: [u8; DIGEST_LEN],
    pub profile_digest: [u8; DIGEST_LEN],
    pub boot_epoch: u64,
    pub validation_request_id: u64,
    pub upload_handle: u64,
    pub profile_len: u32,
    pub provider_signature: [u8; SIGNATURE_LEN],
}

pub trait ProviderCasVerifier {
    fn verify_provider_cas(&self, receipt: &ProviderCasReceipt) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedSignedTime(ValidatedRequest<AcceptSignedTimeRequest>);
impl VerifiedSignedTime {
    pub(crate) const fn request(&self) -> &AcceptSignedTimeRequest {
        self.0.request()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedProfileValidation {
    request: ValidatedRequest<ValidateAndStageRelayProfileRequest>,
    authority_epoch: u64,
    csr_handle: u64,
}
impl AdmittedProfileValidation {
    pub const fn request(&self) -> &ValidateAndStageRelayProfileRequest {
        self.request.request()
    }

    pub const fn authority_epoch(&self) -> u64 {
        self.authority_epoch
    }

    pub const fn csr_handle(&self) -> u64 {
        self.csr_handle
    }

    pub(crate) const fn new(
        request: ValidatedRequest<ValidateAndStageRelayProfileRequest>,
        authority_epoch: u64,
        csr_handle: u64,
    ) -> Self {
        Self {
            request,
            authority_epoch,
            csr_handle,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootValidatedProfile(AdmittedProfileValidation);
impl RootValidatedProfile {
    pub(crate) const fn request(&self) -> &ValidateAndStageRelayProfileRequest {
        self.0.request()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedProviderCasReceipt(ProviderCasReceipt);
impl VerifiedProviderCasReceipt {
    pub(crate) const fn receipt(&self) -> &ProviderCasReceipt {
        &self.0
    }
}

pub fn verify_signed_time<P: RequestAuthenticator, S: SignedTimeVerifier>(
    request: AcceptSignedTimeRequest,
    header: &FrameHeader,
    request_policy: &P,
    source_policy: &S,
) -> Result<VerifiedSignedTime, AuthorityFault> {
    let validated = verify_typed_request(request, header, request_policy)?;
    if !is_strict_p256_der_signature(validated.request().source_signature.as_slice())
        || !source_policy.verify_signed_time(validated.request())
    {
        return Err(AuthorityFault::TimeInvalid);
    }
    Ok(VerifiedSignedTime(validated))
}

pub fn verify_root_profile<R: RootProfileVerifier>(
    admitted: AdmittedProfileValidation,
    root_policy: &R,
) -> Result<RootValidatedProfile, AuthorityFault> {
    if !root_policy.verify_root_profile(&admitted) {
        return Err(AuthorityFault::ProfileRejected);
    }
    Ok(RootValidatedProfile(admitted))
}

pub fn verify_provider_cas_receipt<V: ProviderCasVerifier>(
    receipt: ProviderCasReceipt,
    verifier: &V,
) -> Result<VerifiedProviderCasReceipt, AuthorityFault> {
    if !verifier.verify_provider_cas(&receipt) {
        return Err(AuthorityFault::ProviderSplitBrain);
    }
    Ok(VerifiedProviderCasReceipt(receipt))
}

/// Strict, positive, minimally encoded ASN.1 DER ECDSA-P256 signature.
pub fn is_strict_p256_der_signature(signature: &[u8]) -> bool {
    if !(8..=72).contains(&signature.len())
        || signature[0] != 0x30
        || signature[1] as usize != signature.len() - 2
    {
        return false;
    }
    let mut offset = 2;
    for _ in 0..2 {
        if offset + 2 > signature.len() || signature[offset] != 0x02 {
            return false;
        }
        let length = signature[offset + 1] as usize;
        offset += 2;
        if length == 0 || length > 33 || offset + length > signature.len() {
            return false;
        }
        let integer = &signature[offset..offset + length];
        if integer[0] & 0x80 != 0
            || (length > 1 && integer[0] == 0 && integer[1] & 0x80 == 0)
            || integer.iter().all(|byte| *byte == 0)
        {
            return false;
        }
        offset += length;
    }
    offset == signature.len()
}
