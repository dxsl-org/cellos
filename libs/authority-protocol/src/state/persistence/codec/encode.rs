use super::super::ProtectedAuthorityRecord;
use super::common::Writer;
use crate::{
    AuthorityMode, BootState, ProviderCasReceipt, RelayIntent, RelayProfileState, TimeState,
    WireError,
};

impl ProtectedAuthorityRecord {
    pub fn encode_canonical(&self, output: &mut [u8]) -> Result<usize, WireError> {
        let mut writer = Writer::new(output);
        writer.put(b"ASTR")?;
        writer.u8(1)?;
        writer.u64(self.revision)?;
        writer.u8(match self.mode {
            AuthorityMode::Ready => 1,
            AuthorityMode::Serving => 2,
            AuthorityMode::Sealed => 3,
        })?;
        put_boot(&mut writer, self.boot)?;
        put_time(&mut writer, self.time)?;
        put_relay(&mut writer, self.relay)?;
        writer.put(&self.device_id)?;
        writer.put(&self.authority_id)?;
        writer.u64(self.authority_epoch)?;
        writer.u64(self.boot_floor)?;
        writer.u64(self.generation_floor)?;
        writer.u64(self.state_epoch)?;
        writer.put(&self.approved_loader_digest)?;
        writer.u64(self.last_request_sequence)?;
        match self.previous_active {
            Some(intent) => {
                writer.u8(1)?;
                put_intent(&mut writer, intent)?;
            }
            None => writer.u8(0)?,
        }
        match self.pending_time {
            Some(value) => {
                writer.u8(1)?;
                writer.put(&value.time_request_id)?;
                writer.u8(value.purpose as u8)?;
                writer.put(&value.nonce)?;
            }
            None => writer.u8(0)?,
        }
        writer.u64(self.time_floors.source_epoch)?;
        writer.u64(self.time_floors.source_sequence)?;
        writer.u64(self.time_floors.unix_seconds)?;
        Ok(writer.finish())
    }
}

fn put_boot(writer: &mut Writer<'_>, boot: BootState) -> Result<(), WireError> {
    match boot {
        BootState::Closed => writer.u8(0),
        BootState::Open { epoch } => {
            writer.u8(1)?;
            writer.u64(epoch)
        }
    }
}
fn put_time(writer: &mut Writer<'_>, time: TimeState) -> Result<(), WireError> {
    match time {
        TimeState::Unavailable => writer.u8(0),
        TimeState::Valid {
            source_epoch,
            sequence,
            expires_at,
            time_request_id,
            purpose,
        } => {
            writer.u8(1)?;
            writer.u64(source_epoch)?;
            writer.u64(sequence)?;
            writer.u64(expires_at)?;
            writer.put(&time_request_id)?;
            writer.u8(purpose as u8)
        }
    }
}
fn put_relay(writer: &mut Writer<'_>, relay: RelayProfileState) -> Result<(), WireError> {
    match relay {
        RelayProfileState::Empty => writer.u8(0),
        RelayProfileState::Pending {
            generation,
            csr_handle,
        } => {
            writer.u8(1)?;
            writer.u64(generation)?;
            writer.u64(csr_handle)
        }
        RelayProfileState::Staged(value) => {
            writer.u8(2)?;
            put_intent(writer, value)
        }
        RelayProfileState::ReceiptConsumed(value) => {
            writer.u8(3)?;
            put_intent(writer, value)
        }
        RelayProfileState::Prepared(value) => {
            writer.u8(4)?;
            put_intent(writer, value)
        }
        RelayProfileState::Promoted { intent, receipt } => {
            writer.u8(5)?;
            put_intent(writer, intent)?;
            put_receipt(writer, receipt)
        }
        RelayProfileState::Active(value) => {
            writer.u8(6)?;
            put_intent(writer, value)
        }
    }
}
fn put_intent(writer: &mut Writer<'_>, value: RelayIntent) -> Result<(), WireError> {
    writer.put(&value.device_id)?;
    writer.put(&value.authority_id)?;
    writer.u64(value.authority_epoch)?;
    writer.u64(value.generation)?;
    writer.u64(value.policy_epoch)?;
    writer.u8(value.pending_slot)?;
    writer.put(&value.pending_spki_digest)?;
    writer.put(&value.profile_digest)?;
    writer.u64(value.boot_epoch)?;
    writer.u64(value.validation_request_id)
}
fn put_receipt(writer: &mut Writer<'_>, value: ProviderCasReceipt) -> Result<(), WireError> {
    put_intent(
        writer,
        RelayIntent {
            device_id: value.device_id,
            authority_id: value.authority_id,
            authority_epoch: value.authority_epoch,
            generation: value.generation,
            policy_epoch: value.policy_epoch,
            pending_slot: value.pending_slot,
            pending_spki_digest: value.pending_spki_digest,
            profile_digest: value.profile_digest,
            boot_epoch: value.boot_epoch,
            validation_request_id: value.validation_request_id,
        },
    )?;
    writer.put(&value.provider_signature)
}
