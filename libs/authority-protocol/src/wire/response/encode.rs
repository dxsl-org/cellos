use super::super::{common::write_binding, payload::Writer};
use super::TypedResponse;
use crate::{WireError, DIGEST_LEN};

impl TypedResponse {
    pub(crate) fn encode_payload(&self, output: &mut [u8]) -> Result<usize, WireError> {
        let operation = self.operation();
        let mut writer = Writer::new(output);
        macro_rules! binding {
            ($value:expr) => {
                write_binding(&mut writer, &$value.binding, operation)?
            };
        }
        match self {
            Self::OpenBoot(value) => {
                binding!(value);
                writer.u64(value.boot_epoch)?;
                writer.u64(value.state_epoch)?;
                writer.put(&value.approved_loader_digest)?;
            }
            Self::ReadCommittedRelayState(value) => {
                binding!(value);
                write_relay_tuple(
                    &mut writer,
                    value.generation,
                    value.policy_epoch,
                    &value.profile_digest,
                )?;
            }
            Self::RequestSignedTime(value) => {
                binding!(value);
                writer.put(&value.time_request_id)?;
                writer.u8(value.purpose)?;
                writer.put(&value.nonce)?;
            }
            Self::AcceptSignedTime(value) => {
                binding!(value);
                writer.put(&value.time_request_id)?;
                writer.u8(value.purpose)?;
                writer.u64(value.source_epoch)?;
                writer.u64(value.source_sequence)?;
                writer.u64(value.expires_at)?;
            }
            Self::BeginRelayEnrollment(value) => {
                binding!(value);
                writer.u64(value.generation)?;
                writer.u64(value.policy_epoch)?;
                writer.u64(value.csr_handle)?;
                writer.u32(value.csr_len)?;
                writer.put(&value.csr_digest)?;
            }
            Self::ReadRelayCsrChunk(value) => {
                binding!(value);
                writer.u32(value.chunk_index)?;
                writer.bounded(&value.chunk)?;
            }
            Self::ValidateAndStageRelayProfile(value) => {
                binding!(value);
                writer.put(&value.receipt.device_id)?;
                writer.put(&value.receipt.authority_id)?;
                writer.u64(value.receipt.authority_epoch)?;
                writer.u64(value.receipt.generation)?;
                writer.u64(value.receipt.policy_epoch)?;
                writer.u8(value.receipt.pending_slot)?;
                writer.put(&value.receipt.pending_spki_digest)?;
                writer.put(&value.receipt.profile_digest)?;
                writer.u64(value.receipt.boot_epoch)?;
                writer.u64(value.receipt.validation_request_id)?;
            }
            Self::ConsumeStagedRelayProfile(value) => {
                binding!(value);
                writer.u64(value.generation)?;
            }
            Self::CommitRelayGeneration(value) => {
                binding!(value);
                write_relay_tuple(
                    &mut writer,
                    value.generation,
                    value.policy_epoch,
                    &value.profile_digest,
                )?;
            }
            Self::AbortRelayEnrollment(value) => {
                binding!(value);
                writer.u64(value.generation)?;
            }
            Self::GetRelayActivePublicKey(value) => {
                binding!(value);
                writer.u64(value.generation)?;
                writer.put(&value.public_key)?;
                writer.put(&value.public_key_digest)?;
            }
            Self::SignTls13ClientCertificateVerify(value) => {
                binding!(value);
                writer.put(&value.signature)?;
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
