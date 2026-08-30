use super::{io::Writer, CodecError, MAGIC, TAG_LEN, VERSION};
use crate::{FullRecord, HardwareBindings, ProfileMaterial, RecordAuthenticator};
use authority_protocol::{WireError, PROTECTED_RECORD_MAX};

/// Encode and authenticate one full record into `output`.
pub fn encode_record<A: RecordAuthenticator>(
    record: &FullRecord,
    authenticator: &A,
    output: &mut [u8],
) -> Result<usize, CodecError> {
    record.validate()?;
    let mut protected = [0u8; PROTECTED_RECORD_MAX];
    let protected_len = record.protected.encode_canonical(&mut protected)?;
    let body_len = {
        let mut writer = Writer::new(output);
        writer.put(MAGIC)?;
        writer.u8(VERSION)?;
        writer.u8(record.slot_role as u8)?;
        writer.u16(0)?;
        writer.u64(record.counter)?;
        put_hardware(&mut writer, &record.hardware)?;
        writer.u16(length(protected_len)?)?;
        writer.put(&protected[..protected_len])?;
        put_profile(&mut writer, record.active.as_ref())?;
        put_profile(&mut writer, record.pending.as_ref())?;
        writer.len()
    };
    let end = body_len
        .checked_add(TAG_LEN)
        .ok_or(WireError::BufferTooSmall)?;
    if end > output.len() {
        return Err(WireError::BufferTooSmall.into());
    }
    let tag = authenticator.authenticate(&output[..body_len]);
    output[body_len..end].copy_from_slice(&tag);
    Ok(end)
}

fn put_hardware(writer: &mut Writer<'_>, value: &HardwareBindings) -> Result<(), WireError> {
    writer.put(&value.lane_id)?;
    writer.u64(value.restart_floor)?;
    writer.put(&value.approved_boot_measurement)?;
    writer.put(&value.approved_loader_digest)?;
    writer.put(&value.manifest_key_digest)?;
    writer.u64(value.firmware_floor)?;
    writer.u64(value.policy_floor)?;
    writer.put(&value.trust_digest)?;
    writer.put(&value.verifier_digest)?;
    writer.put(&value.denylist_digest)?;
    writer.put(&value.qualification_digest)
}

fn put_profile(writer: &mut Writer<'_>, value: Option<&ProfileMaterial>) -> Result<(), WireError> {
    let Some(value) = value else {
        return writer.u8(0);
    };
    writer.u8(1)?;
    writer.put(&value.device_id)?;
    writer.put(&value.authority_id)?;
    writer.u64(value.authority_epoch)?;
    writer.u64(value.boot_epoch)?;
    writer.u8(value.slot)?;
    writer.u64(value.generation)?;
    writer.u32(value.profile_len)?;
    writer.put(&value.profile_digest)?;
    writer.put(&value.tpm_public_digest)?;
    writer.u16(length(value.spki.len())?)?;
    writer.put(value.spki.as_slice())
}

fn length(value: usize) -> Result<u16, WireError> {
    value.try_into().map_err(|_| WireError::OversizePayload)
}
