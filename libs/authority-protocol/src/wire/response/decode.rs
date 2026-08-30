use super::super::{common::read_binding, payload::Reader};
use super::TypedResponse;
use crate::*;

impl TypedResponse {
    pub fn decode_payload(operation: Operation, input: &[u8]) -> Result<Self, WireError> {
        let mut reader = Reader::new(input);
        let binding = read_binding(&mut reader, operation)?;
        let response = match operation {
            Operation::OpenBoot => Self::OpenBoot(OpenBootResponse {
                binding,
                boot_epoch: reader.u64()?,
                state_epoch: reader.u64()?,
                approved_loader_digest: reader.array()?,
            }),
            Operation::ReadCommittedRelayState => {
                let (generation, policy_epoch, profile_digest) = read_relay_tuple(&mut reader)?;
                Self::ReadCommittedRelayState(ReadCommittedRelayStateResponse {
                    binding,
                    generation,
                    policy_epoch,
                    profile_digest,
                })
            }
            Operation::RequestSignedTime => Self::RequestSignedTime(RequestSignedTimeResponse {
                binding,
                time_request_id: reader.array()?,
                purpose: read_purpose(&mut reader)?,
                nonce: reader.array()?,
            }),
            Operation::AcceptSignedTime => Self::AcceptSignedTime(AcceptSignedTimeResponse {
                binding,
                time_request_id: reader.array()?,
                purpose: read_purpose(&mut reader)?,
                source_epoch: reader.u64()?,
                source_sequence: reader.u64()?,
                expires_at: reader.u64()?,
            }),
            Operation::BeginRelayEnrollment => {
                let value = BeginRelayEnrollmentResponse {
                    binding,
                    generation: reader.u64()?,
                    policy_epoch: reader.u64()?,
                    pending_slot: reader.u8()?,
                    csr_handle: reader.u64()?,
                    csr_len: reader.u32()?,
                    csr_digest: reader.array()?,
                };
                if value.pending_slot > 1 {
                    return Err(WireError::InvalidLength);
                }
                Self::BeginRelayEnrollment(value)
            }
            Operation::ReadRelayCsrChunk => Self::ReadRelayCsrChunk(ReadRelayCsrChunkResponse {
                binding,
                chunk_index: reader.u32()?,
                chunk: reader.bounded()?,
            }),
            Operation::ValidateAndStageRelayProfile => {
                let receipt = StagedProfileReceipt {
                    device_id: reader.array()?,
                    authority_id: reader.array()?,
                    authority_epoch: reader.u64()?,
                    generation: reader.u64()?,
                    policy_epoch: reader.u64()?,
                    pending_slot: reader.u8()?,
                    pending_spki_digest: reader.array()?,
                    profile_digest: reader.array()?,
                    boot_epoch: reader.u64()?,
                    upload_handle: reader.u64()?,
                    profile_len: reader.u32()?,
                    validation_request_id: reader.u64()?,
                };
                if receipt.pending_slot > 1
                    || receipt.upload_handle == 0
                    || receipt.profile_len == 0
                    || receipt.profile_len as usize > PROFILE_MAX_LEN
                {
                    return Err(WireError::InvalidLength);
                }
                Self::ValidateAndStageRelayProfile(ValidateAndStageRelayProfileResponse {
                    binding,
                    receipt,
                })
            }
            Operation::ConsumeStagedRelayProfile => {
                Self::ConsumeStagedRelayProfile(ConsumeStagedRelayProfileResponse {
                    binding,
                    generation: reader.u64()?,
                })
            }
            Operation::CommitRelayGeneration => {
                let (generation, policy_epoch, profile_digest) = read_relay_tuple(&mut reader)?;
                Self::CommitRelayGeneration(CommitRelayGenerationResponse {
                    binding,
                    generation,
                    policy_epoch,
                    profile_digest,
                })
            }
            Operation::AbortRelayEnrollment => {
                Self::AbortRelayEnrollment(AbortRelayEnrollmentResponse {
                    binding,
                    generation: reader.u64()?,
                })
            }
            Operation::GetRelayActivePublicKey => {
                let value = GetRelayActivePublicKeyResponse {
                    binding,
                    generation: reader.u64()?,
                    public_key: reader.array()?,
                    public_key_digest: reader.array()?,
                };
                if value.public_key[0] != 4 {
                    return Err(WireError::InvalidLength);
                }
                Self::GetRelayActivePublicKey(value)
            }
            Operation::SignTls13ClientCertificateVerify => {
                Self::SignTls13ClientCertificateVerify(SignTls13ClientCertificateVerifyResponse {
                    binding,
                    signature: reader.array()?,
                })
            }
            Operation::BeginRelayProfileUpload => {
                let value = BeginRelayProfileUploadResponse {
                    binding,
                    upload_handle: reader.u64()?,
                    profile_len: reader.u32()?,
                    chunk_size: reader.u16()?,
                    next_index: reader.u8()?,
                };
                let size = PROFILE_CHUNK_MAX as u32;
                let chunks = value.profile_len.div_ceil(size);
                if value.upload_handle == 0
                    || value.profile_len == 0
                    || value.profile_len as usize > PROFILE_MAX_LEN
                    || value.chunk_size as usize != PROFILE_CHUNK_MAX
                    || value.next_index as u32 > chunks
                {
                    return Err(WireError::InvalidLength);
                }
                Self::BeginRelayProfileUpload(value)
            }
            Operation::WriteRelayProfileChunk => {
                let value = WriteRelayProfileChunkResponse {
                    binding,
                    upload_handle: reader.u64()?,
                    next_index: reader.u8()?,
                    complete: reader.u8()?,
                };
                if value.upload_handle == 0
                    || value.next_index as usize > PROFILE_MAX_CHUNKS
                    || value.complete > 1
                {
                    return Err(WireError::InvalidLength);
                }
                Self::WriteRelayProfileChunk(value)
            }
        };
        reader.finish()?;
        Ok(response)
    }
}

fn read_purpose(reader: &mut Reader<'_>) -> Result<u8, WireError> {
    let value = reader.u8()?;
    TimePurpose::try_from(value).map_err(|_| WireError::InvalidLength)?;
    Ok(value)
}

fn read_relay_tuple(reader: &mut Reader<'_>) -> Result<(u64, u64, [u8; DIGEST_LEN]), WireError> {
    Ok((reader.u64()?, reader.u64()?, reader.array()?))
}
