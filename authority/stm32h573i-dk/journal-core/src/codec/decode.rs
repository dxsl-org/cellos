use super::{io::Reader, CodecError, MAGIC, RECORD_MAX, TAG_LEN, VERSION};
use crate::{
    FullRecord, HardwareBindings, ProfileMaterial, RecordAuthenticator, SlotRole, SPKI_MAX,
};
use authority_protocol::{
    constant_time_eq, verify_protected_record, Bounded, ProtectedAuthorityRecord,
    ProtectedRecordVerifier, WireError, PROFILE_MAX,
};

struct AuthenticatedRecord;

impl ProtectedRecordVerifier for AuthenticatedRecord {
    fn verify(&self, _record: &ProtectedAuthorityRecord) -> bool {
        true
    }
}

/// Authenticate, exactly decode, and validate one full record.
pub fn decode_record<A: RecordAuthenticator>(
    input: &[u8],
    authenticator: &A,
) -> Result<FullRecord, CodecError> {
    if input.len() > RECORD_MAX {
        return Err(WireError::OversizePayload.into());
    }
    let body_len = input
        .len()
        .checked_sub(TAG_LEN)
        .ok_or(WireError::Truncated)?;
    let expected = authenticator.authenticate(&input[..body_len]);
    if !constant_time_eq(&expected, &input[body_len..]) {
        return Err(CodecError::Authentication);
    }
    let mut reader = Reader::new(&input[..body_len]);
    if reader.take(4)? != MAGIC {
        return Err(WireError::BadMagic.into());
    }
    if reader.u8()? != VERSION {
        return Err(WireError::UnsupportedVersion.into());
    }
    let slot_role = match reader.u8()? {
        0 => SlotRole::A,
        1 => SlotRole::B,
        _ => return Err(WireError::UnknownMessageKind.into()),
    };
    if reader.u16()? != 0 {
        return Err(WireError::NonZeroReserved.into());
    }
    let counter = reader.u64()?;
    let hardware = get_hardware(&mut reader)?;
    let protected_len = reader.u16()? as usize;
    let protected = ProtectedAuthorityRecord::decode_canonical(reader.take(protected_len)?)?;
    let active = get_profile(&mut reader)?;
    let pending = get_profile(&mut reader)?;
    if reader.remaining() != 0 {
        return Err(WireError::TrailingBytes.into());
    }
    verify_protected_record(protected, &AuthenticatedRecord)
        .map_err(|_| CodecError::ProtectedRecord)?;
    let record = FullRecord {
        counter,
        slot_role,
        hardware,
        protected,
        active,
        pending,
    };
    record.validate()?;
    Ok(record)
}

fn get_hardware(reader: &mut Reader<'_>) -> Result<HardwareBindings, WireError> {
    Ok(HardwareBindings {
        lane_id: reader.array()?,
        restart_floor: reader.u64()?,
        approved_boot_measurement: reader.array()?,
        approved_loader_digest: reader.array()?,
        manifest_key_digest: reader.array()?,
        firmware_floor: reader.u64()?,
        policy_floor: reader.u64()?,
        trust_digest: reader.array()?,
        verifier_digest: reader.array()?,
        denylist_digest: reader.array()?,
        qualification_digest: reader.array()?,
    })
}

fn get_profile(reader: &mut Reader<'_>) -> Result<Option<ProfileMaterial>, WireError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => {
            let slot = reader.u8()?;
            let spki_len = reader.u16()? as usize;
            let spki = Bounded::<SPKI_MAX>::from_slice(reader.take(spki_len)?)
                .ok_or(WireError::OversizePayload)?;
            let profile_len = reader.u16()? as usize;
            let profile = Bounded::<PROFILE_MAX>::from_slice(reader.take(profile_len)?)
                .ok_or(WireError::OversizePayload)?;
            Ok(Some(ProfileMaterial {
                slot,
                spki,
                profile,
            }))
        }
        _ => Err(WireError::UnknownMessageKind),
    }
}
