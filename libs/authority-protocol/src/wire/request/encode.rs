use super::*;
use crate::*;

impl TypedRequest {
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
