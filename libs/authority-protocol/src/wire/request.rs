mod decode;
mod encode;

use super::{common::*, payload::*};
use crate::*;

// Fixed inline storage is the no-allocation wire contract; boxing is forbidden.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedRequest {
    OpenBoot(OpenBootRequest),
    ReadCommittedRelayState(ReadCommittedRelayStateRequest),
    RequestSignedTime(RequestSignedTimeRequest),
    AcceptSignedTime(AcceptSignedTimeRequest),
    BeginRelayEnrollment(BeginRelayEnrollmentRequest),
    ReadRelayCsrChunk(ReadRelayCsrChunkRequest),
    ValidateAndStageRelayProfile(ValidateAndStageRelayProfileRequest),
    ConsumeStagedRelayProfile(ConsumeStagedRelayProfileRequest),
    CommitRelayGeneration(CommitRelayGenerationRequest),
    AbortRelayEnrollment(AbortRelayEnrollmentRequest),
    GetRelayActivePublicKey(GetRelayActivePublicKeyRequest),
    SignTls13ClientCertificateVerify(SignTls13ClientCertificateVerifyRequest),
}
pub(crate) trait CanonicalBody {
    fn canonical_body(&self) -> ([u8; DIGEST_LEN], usize);
}

macro_rules! canonical_body {
    ($( $type:ty => $variant:ident ),+ $(,)?) => {$(
        impl CanonicalBody for $type {
            fn canonical_body(&self) -> ([u8; DIGEST_LEN], usize) {
                let request = TypedRequest::$variant(*self);
                request.canonical_body()
            }
        }
    )+};
}

canonical_body! {
    OpenBootRequest => OpenBoot,
    ReadCommittedRelayStateRequest => ReadCommittedRelayState,
    RequestSignedTimeRequest => RequestSignedTime,
    AcceptSignedTimeRequest => AcceptSignedTime,
    BeginRelayEnrollmentRequest => BeginRelayEnrollment,
    ReadRelayCsrChunkRequest => ReadRelayCsrChunk,
    ValidateAndStageRelayProfileRequest => ValidateAndStageRelayProfile,
    ConsumeStagedRelayProfileRequest => ConsumeStagedRelayProfile,
    CommitRelayGenerationRequest => CommitRelayGeneration,
    AbortRelayEnrollmentRequest => AbortRelayEnrollment,
    GetRelayActivePublicKeyRequest => GetRelayActivePublicKey,
    SignTls13ClientCertificateVerifyRequest => SignTls13ClientCertificateVerify,
}

impl TypedRequest {
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
    pub const fn context(&self) -> &RequestContext {
        match self {
            Self::OpenBoot(value) => &value.context,
            Self::ReadCommittedRelayState(value) => &value.context,
            Self::RequestSignedTime(value) => &value.context,
            Self::AcceptSignedTime(value) => &value.context,
            Self::BeginRelayEnrollment(value) => &value.context,
            Self::ReadRelayCsrChunk(value) => &value.context,
            Self::ValidateAndStageRelayProfile(value) => &value.context,
            Self::ConsumeStagedRelayProfile(value) => &value.context,
            Self::CommitRelayGeneration(value) => &value.context,
            Self::AbortRelayEnrollment(value) => &value.context,
            Self::GetRelayActivePublicKey(value) => &value.context,
            Self::SignTls13ClientCertificateVerify(value) => &value.context,
        }
    }

    pub fn context_mut(&mut self) -> &mut RequestContext {
        match self {
            Self::OpenBoot(value) => &mut value.context,
            Self::ReadCommittedRelayState(value) => &mut value.context,
            Self::RequestSignedTime(value) => &mut value.context,
            Self::AcceptSignedTime(value) => &mut value.context,
            Self::BeginRelayEnrollment(value) => &mut value.context,
            Self::ReadRelayCsrChunk(value) => &mut value.context,
            Self::ValidateAndStageRelayProfile(value) => &mut value.context,
            Self::ConsumeStagedRelayProfile(value) => &mut value.context,
            Self::CommitRelayGeneration(value) => &mut value.context,
            Self::AbortRelayEnrollment(value) => &mut value.context,
            Self::GetRelayActivePublicKey(value) => &mut value.context,
            Self::SignTls13ClientCertificateVerify(value) => &mut value.context,
        }
    }

    pub fn canonical_body_digest(&self) -> [u8; DIGEST_LEN] {
        self.canonical_body().0
    }

    pub fn canonical_payload_len(&self) -> usize {
        self.canonical_body().1
    }

    fn canonical_body(&self) -> ([u8; DIGEST_LEN], usize) {
        let mut payload = [0u8; FRAME_MAX_PAYLOAD];
        let length = self
            .encode_payload(&mut payload)
            .expect("typed request fits protocol maximum");
        (
            crate::sha256(&payload[REQUEST_CONTEXT_WIRE_LEN..length]),
            length,
        )
    }
}
