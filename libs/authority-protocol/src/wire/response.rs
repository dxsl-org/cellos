mod decode;
mod encode;

use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedResponse {
    OpenBoot(OpenBootResponse),
    ReadCommittedRelayState(ReadCommittedRelayStateResponse),
    RequestSignedTime(RequestSignedTimeResponse),
    AcceptSignedTime(AcceptSignedTimeResponse),
    BeginRelayEnrollment(BeginRelayEnrollmentResponse),
    ReadRelayCsrChunk(ReadRelayCsrChunkResponse),
    ValidateAndStageRelayProfile(ValidateAndStageRelayProfileResponse),
    ConsumeStagedRelayProfile(ConsumeStagedRelayProfileResponse),
    CommitRelayGeneration(CommitRelayGenerationResponse),
    AbortRelayEnrollment(AbortRelayEnrollmentResponse),
    GetRelayActivePublicKey(GetRelayActivePublicKeyResponse),
    SignTls13ClientCertificateVerify(SignTls13ClientCertificateVerifyResponse),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatedResponse(TypedResponse);

impl ValidatedResponse {
    pub const fn response(&self) -> &TypedResponse {
        &self.0
    }
}
pub fn verify_typed_response<A: ResponseAuthenticator>(
    response: TypedResponse,
    header: &FrameHeader,
    expected: &ExpectedResponseBinding,
    authenticator: &A,
) -> Result<ValidatedResponse, AuthorityFault> {
    let binding = response.binding();
    let mut payload = [0u8; FRAME_MAX_PAYLOAD];
    let length = response
        .encode_payload(&mut payload)
        .map_err(|_| AuthorityFault::Malformed)?;
    let body_digest = crate::sha256(&payload[RESPONSE_BINDING_WIRE_LEN..length]);
    if header.class != FrameClass::Response
        || header.operation != response.operation()
        || header.payload_len as usize != length
        || response.operation() != expected.operation
        || header.request_id != expected.request_id
        || binding.device_id != expected.device_id
        || binding.authority_id != expected.authority_id
        || binding.boot_epoch != expected.boot_epoch
        || binding.request_id != expected.request_id
        || binding.operation != response.operation()
        || !constant_time_eq(&binding.payload_digest, &body_digest)
        || !constant_time_eq(&binding.authority_signature[..16], &header.authenticator)
        || !authenticator.verify(
            &binding.authentication_input(),
            &binding.authority_signature,
        )
    {
        return Err(AuthorityFault::ChallengeMismatch);
    }
    Ok(ValidatedResponse(response))
}

impl TypedResponse {
    pub const fn operation(&self) -> Operation {
        match self {
            Self::OpenBoot(_) => Operation::OpenBoot,
            Self::ReadCommittedRelayState(_) => Operation::ReadCommittedRelayState,
            Self::RequestSignedTime(_) => Operation::RequestSignedTime,
            Self::AcceptSignedTime(_) => Operation::AcceptSignedTime,
            Self::BeginRelayEnrollment(_) => Operation::BeginRelayEnrollment,
            Self::ReadRelayCsrChunk(_) => Operation::ReadRelayCsrChunk,
            Self::ValidateAndStageRelayProfile(_) => Operation::ValidateAndStageRelayProfile,
            Self::ConsumeStagedRelayProfile(_) => Operation::ConsumeStagedRelayProfile,
            Self::CommitRelayGeneration(_) => Operation::CommitRelayGeneration,
            Self::AbortRelayEnrollment(_) => Operation::AbortRelayEnrollment,
            Self::GetRelayActivePublicKey(_) => Operation::GetRelayActivePublicKey,
            Self::SignTls13ClientCertificateVerify(_) => {
                Operation::SignTls13ClientCertificateVerify
            }
        }
    }
    pub const fn binding(&self) -> &AuthenticatedBinding {
        match self {
            Self::OpenBoot(value) => &value.binding,
            Self::ReadCommittedRelayState(value) => &value.binding,
            Self::RequestSignedTime(value) => &value.binding,
            Self::AcceptSignedTime(value) => &value.binding,
            Self::BeginRelayEnrollment(value) => &value.binding,
            Self::ReadRelayCsrChunk(value) => &value.binding,
            Self::ValidateAndStageRelayProfile(value) => &value.binding,
            Self::ConsumeStagedRelayProfile(value) => &value.binding,
            Self::CommitRelayGeneration(value) => &value.binding,
            Self::AbortRelayEnrollment(value) => &value.binding,
            Self::GetRelayActivePublicKey(value) => &value.binding,
            Self::SignTls13ClientCertificateVerify(value) => &value.binding,
        }
    }

    pub fn binding_mut(&mut self) -> &mut AuthenticatedBinding {
        match self {
            Self::OpenBoot(value) => &mut value.binding,
            Self::ReadCommittedRelayState(value) => &mut value.binding,
            Self::RequestSignedTime(value) => &mut value.binding,
            Self::AcceptSignedTime(value) => &mut value.binding,
            Self::BeginRelayEnrollment(value) => &mut value.binding,
            Self::ReadRelayCsrChunk(value) => &mut value.binding,
            Self::ValidateAndStageRelayProfile(value) => &mut value.binding,
            Self::ConsumeStagedRelayProfile(value) => &mut value.binding,
            Self::CommitRelayGeneration(value) => &mut value.binding,
            Self::AbortRelayEnrollment(value) => &mut value.binding,
            Self::GetRelayActivePublicKey(value) => &mut value.binding,
            Self::SignTls13ClientCertificateVerify(value) => &mut value.binding,
        }
    }

    pub fn canonical_body_digest(&self) -> [u8; DIGEST_LEN] {
        let mut payload = [0u8; FRAME_MAX_PAYLOAD];
        let length = self
            .encode_payload(&mut payload)
            .expect("typed response fits protocol maximum");
        crate::sha256(&payload[RESPONSE_BINDING_WIRE_LEN..length])
    }
}
