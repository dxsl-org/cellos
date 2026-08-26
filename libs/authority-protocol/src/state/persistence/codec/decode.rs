use super::super::ProtectedAuthorityRecord;
use super::common::Reader;
use crate::{
    AuthorityMode, BootState, PendingTimeChallenge, ProtectedTimeFloors, ProviderCasReceipt,
    RelayIntent, RelayProfileState, TimePurpose, TimeState, WireError,
};

impl ProtectedAuthorityRecord {
    pub fn decode_canonical(input: &[u8]) -> Result<Self, WireError> {
        let mut reader = Reader::new(input);
        if reader.take(4)? != b"ASTR" {
            return Err(WireError::BadMagic);
        }
        if reader.u8()? != 1 {
            return Err(WireError::UnsupportedVersion);
        }
        let revision = reader.u64()?;
        let mode = match reader.u8()? {
            1 => AuthorityMode::Ready,
            2 => AuthorityMode::Serving,
            3 => AuthorityMode::Sealed,
            _ => return Err(WireError::UnknownMessageKind),
        };
        let boot = get_boot(&mut reader)?;
        let time = get_time(&mut reader)?;
        let relay = get_relay(&mut reader)?;
        let device_id = reader.array()?;
        let authority_id = reader.array()?;
        let authority_epoch = reader.u64()?;
        let boot_floor = reader.u64()?;
        let generation_floor = reader.u64()?;
        let state_epoch = reader.u64()?;
        let approved_loader_digest = reader.array()?;
        let last_request_sequence = reader.u64()?;
        let previous_active = match tag(&mut reader)? {
            false => None,
            true => Some(get_intent(&mut reader)?),
        };
        let pending_time = match tag(&mut reader)? {
            false => None,
            true => Some(PendingTimeChallenge {
                time_request_id: reader.array()?,
                purpose: get_purpose(&mut reader)?,
                nonce: reader.array()?,
            }),
        };
        let time_floors = ProtectedTimeFloors {
            source_epoch: reader.u64()?,
            source_sequence: reader.u64()?,
            unix_seconds: reader.u64()?,
        };
        reader.finish()?;
        Ok(Self {
            revision,
            mode,
            boot,
            time,
            relay,
            device_id,
            authority_id,
            authority_epoch,
            boot_floor,
            generation_floor,
            state_epoch,
            approved_loader_digest,
            last_request_sequence,
            previous_active,
            pending_time,
            time_floors,
        })
    }
}

fn tag(reader: &mut Reader<'_>) -> Result<bool, WireError> {
    match reader.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(WireError::UnknownMessageKind),
    }
}
fn get_boot(reader: &mut Reader<'_>) -> Result<BootState, WireError> {
    match reader.u8()? {
        0 => Ok(BootState::Closed),
        1 => Ok(BootState::Open {
            epoch: reader.u64()?,
        }),
        _ => Err(WireError::UnknownMessageKind),
    }
}
fn get_time(reader: &mut Reader<'_>) -> Result<TimeState, WireError> {
    match reader.u8()? {
        0 => Ok(TimeState::Unavailable),
        1 => Ok(TimeState::Valid {
            source_epoch: reader.u64()?,
            sequence: reader.u64()?,
            expires_at: reader.u64()?,
            time_request_id: reader.array()?,
            purpose: get_purpose(reader)?,
        }),
        _ => Err(WireError::UnknownMessageKind),
    }
}
fn get_purpose(reader: &mut Reader<'_>) -> Result<TimePurpose, WireError> {
    TimePurpose::try_from(reader.u8()?).map_err(|_| WireError::UnknownMessageKind)
}
fn get_relay(reader: &mut Reader<'_>) -> Result<RelayProfileState, WireError> {
    Ok(match reader.u8()? {
        0 => RelayProfileState::Empty,
        1 => RelayProfileState::Pending {
            generation: reader.u64()?,
            csr_handle: reader.u64()?,
        },
        2 => RelayProfileState::Staged(get_intent(reader)?),
        3 => RelayProfileState::ReceiptConsumed(get_intent(reader)?),
        4 => RelayProfileState::Prepared(get_intent(reader)?),
        5 => RelayProfileState::Promoted {
            intent: get_intent(reader)?,
            receipt: get_receipt(reader)?,
        },
        6 => RelayProfileState::Active(get_intent(reader)?),
        _ => return Err(WireError::UnknownMessageKind),
    })
}
fn get_intent(reader: &mut Reader<'_>) -> Result<RelayIntent, WireError> {
    Ok(RelayIntent {
        device_id: reader.array()?,
        authority_id: reader.array()?,
        authority_epoch: reader.u64()?,
        generation: reader.u64()?,
        policy_epoch: reader.u64()?,
        pending_slot: reader.u8()?,
        pending_spki_digest: reader.array()?,
        profile_digest: reader.array()?,
        boot_epoch: reader.u64()?,
        validation_request_id: reader.u64()?,
    })
}
fn get_receipt(reader: &mut Reader<'_>) -> Result<ProviderCasReceipt, WireError> {
    let value = get_intent(reader)?;
    Ok(ProviderCasReceipt {
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
        provider_signature: reader.array()?,
    })
}
