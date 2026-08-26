use super::super::{common::read_context, payload::Reader};
use super::TypedRequest;
use crate::*;

impl TypedRequest {
    pub fn decode_payload(operation: Operation, input: &[u8]) -> Result<Self, WireError> {
        let mut reader = Reader::new(input);
        let context = read_context(&mut reader, operation)?;
        let request = match operation {
            Operation::OpenBoot => Self::OpenBoot(OpenBootRequest {
                context,
                loader_digest: reader.array()?,
            }),
            Operation::ReadCommittedRelayState => {
                Self::ReadCommittedRelayState(ReadCommittedRelayStateRequest { context })
            }
            Operation::RequestSignedTime => Self::RequestSignedTime(RequestSignedTimeRequest {
                context,
                purpose: read_purpose(&mut reader)?,
            }),
            Operation::AcceptSignedTime => {
                Self::AcceptSignedTime(read_accept_time(context, &mut reader)?)
            }
            Operation::BeginRelayEnrollment => {
                Self::BeginRelayEnrollment(BeginRelayEnrollmentRequest {
                    context,
                    hostname: reader.bounded()?,
                })
            }
            Operation::ReadRelayCsrChunk => Self::ReadRelayCsrChunk(ReadRelayCsrChunkRequest {
                context,
                csr_handle: reader.u64()?,
                chunk_index: reader.u32()?,
            }),
            Operation::ValidateAndStageRelayProfile => {
                Self::ValidateAndStageRelayProfile(read_profile(context, &mut reader)?)
            }
            Operation::ConsumeStagedRelayProfile => {
                let (generation, policy_epoch, profile_digest) = read_relay_tuple(&mut reader)?;
                Self::ConsumeStagedRelayProfile(ConsumeStagedRelayProfileRequest {
                    context,
                    generation,
                    policy_epoch,
                    profile_digest,
                })
            }
            Operation::CommitRelayGeneration => {
                let (generation, policy_epoch, profile_digest) = read_relay_tuple(&mut reader)?;
                Self::CommitRelayGeneration(CommitRelayGenerationRequest {
                    context,
                    generation,
                    policy_epoch,
                    profile_digest,
                })
            }
            Operation::AbortRelayEnrollment => {
                Self::AbortRelayEnrollment(AbortRelayEnrollmentRequest {
                    context,
                    generation: reader.u64()?,
                })
            }
            Operation::GetRelayActivePublicKey => {
                Self::GetRelayActivePublicKey(GetRelayActivePublicKeyRequest { context })
            }
            Operation::SignTls13ClientCertificateVerify => {
                Self::SignTls13ClientCertificateVerify(SignTls13ClientCertificateVerifyRequest {
                    context,
                    transcript_hash: reader.array()?,
                    relay_generation: reader.u64()?,
                    active_profile_digest: reader.array()?,
                    public_request_id: reader.u64()?,
                })
            }
        };
        reader.finish()?;
        Ok(request)
    }
}

fn read_accept_time(
    context: RequestContext,
    reader: &mut Reader<'_>,
) -> Result<AcceptSignedTimeRequest, WireError> {
    let value = AcceptSignedTimeRequest {
        context,
        time_request_id: reader.array()?,
        purpose: read_purpose(reader)?,
        source_epoch: reader.u64()?,
        source_sequence: reader.u64()?,
        unix_seconds: reader.i64()?,
        expires_at: reader.u64()?,
        nonce: reader.array()?,
        source_signature: reader.bounded()?,
    };
    if !is_strict_p256_der_signature(value.source_signature.as_slice()) {
        return Err(WireError::InvalidLength);
    }
    Ok(value)
}

fn read_profile(
    context: RequestContext,
    reader: &mut Reader<'_>,
) -> Result<ValidateAndStageRelayProfileRequest, WireError> {
    let value = ValidateAndStageRelayProfileRequest {
        context,
        generation: reader.u64()?,
        policy_epoch: reader.u64()?,
        pending_slot: reader.u8()?,
        pending_spki_digest: reader.array()?,
        profile_digest: reader.array()?,
        tpm_public_digest: reader.array()?,
        profile: reader.bounded()?,
    };
    if value.pending_slot > 1 || value.profile.is_empty() {
        return Err(WireError::InvalidLength);
    }
    Ok(value)
}

fn read_purpose(reader: &mut Reader<'_>) -> Result<u8, WireError> {
    let value = reader.u8()?;
    TimePurpose::try_from(value).map_err(|_| WireError::InvalidLength)?;
    Ok(value)
}

fn read_relay_tuple(reader: &mut Reader<'_>) -> Result<(u64, u64, [u8; DIGEST_LEN]), WireError> {
    Ok((reader.u64()?, reader.u64()?, reader.array()?))
}
