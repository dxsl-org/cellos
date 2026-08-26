mod decode;

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

    pub fn encode_payload(&self, output: &mut [u8]) -> Result<usize, WireError> {
        let operation = self.operation();
        let mut writer = Writer::new(output);
        macro_rules! context {
            ($value:expr) => {
                write_context(&mut writer, &$value.context, operation)?
            };
        }
        match self {
            Self::OpenBoot(value) => {
                context!(value);
                writer.put(&value.loader_digest)?;
            }
            Self::ReadCommittedRelayState(value) => context!(value),
            Self::RequestSignedTime(value) => {
                context!(value);
                writer.u8(value.purpose)?;
            }
            Self::AcceptSignedTime(value) => {
                context!(value);
                writer.put(&value.time_request_id)?;
                writer.u8(value.purpose)?;
                writer.u64(value.source_epoch)?;
                writer.u64(value.source_sequence)?;
                writer.i64(value.unix_seconds)?;
                writer.u64(value.expires_at)?;
                writer.put(&value.nonce)?;
                writer.bounded(&value.source_signature)?;
            }
            Self::BeginRelayEnrollment(value) => {
                context!(value);
                writer.bounded(&value.hostname)?;
            }
            Self::ReadRelayCsrChunk(value) => {
                context!(value);
                writer.u64(value.csr_handle)?;
                writer.u32(value.chunk_index)?;
            }
            Self::ValidateAndStageRelayProfile(value) => {
                context!(value);
                writer.u64(value.generation)?;
                writer.u64(value.policy_epoch)?;
                writer.u8(value.pending_slot)?;
                writer.put(&value.pending_spki_digest)?;
                writer.put(&value.profile_digest)?;
                writer.put(&value.tpm_public_digest)?;
                writer.bounded(&value.profile)?;
            }
            Self::ConsumeStagedRelayProfile(value) => {
                context!(value);
                write_relay_tuple(
                    &mut writer,
                    value.generation,
                    value.policy_epoch,
                    &value.profile_digest,
                )?;
            }
            Self::CommitRelayGeneration(value) => {
                context!(value);
                write_relay_tuple(
                    &mut writer,
                    value.generation,
                    value.policy_epoch,
                    &value.profile_digest,
                )?;
            }
            Self::AbortRelayEnrollment(value) => {
                context!(value);
                writer.u64(value.generation)?;
            }
            Self::GetRelayActivePublicKey(value) => context!(value),
            Self::SignTls13ClientCertificateVerify(value) => {
                context!(value);
                writer.put(&value.transcript_hash)?;
                writer.u64(value.relay_generation)?;
                writer.put(&value.active_profile_digest)?;
                writer.u64(value.public_request_id)?;
            }
        }
        Ok(writer.finish())
    }
}

fn write_relay_tuple(
    writer: &mut Writer<'_>,
    generation: u64,
    policy_epoch: u64,
    digest: &[u8; DIGEST_LEN],
) -> Result<(), WireError> {
    writer.u64(generation)?;
    writer.u64(policy_epoch)?;
    writer.put(digest)
}
