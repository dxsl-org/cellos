use super::{ProfileBankAuthenticator, ProfileBankMetadata, CHUNK_RECORD_MAX, HEADER_MAX};
use crate::codec::io::{Reader, Writer};
use crate::SPKI_MAX;
use authority_protocol::{constant_time_eq, Bounded, WireError, DIGEST_LEN};
use sha2::{Digest, Sha256};

const HEADER_MAGIC: &[u8; 4] = b"SPBH";
const CHUNK_MAGIC: &[u8; 4] = b"SPBC";
const VERSION: u8 = 2;
const TAG_LEN: usize = DIGEST_LEN;

pub(crate) struct Chunk<'a> {
    pub slot: u8,
    pub index: u8,
    pub metadata_digest: [u8; DIGEST_LEN],
    pub bytes: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChunkDecodeError {
    Unauthenticated,
    Malformed,
}

pub(crate) fn metadata_digest(value: &ProfileBankMetadata) -> [u8; DIGEST_LEN] {
    let mut hash = Sha256::new();
    hash.update(HEADER_MAGIC);
    hash.update([VERSION, value.slot]);
    hash.update(value.device_id);
    hash.update(value.authority_id);
    hash.update(value.authority_epoch.to_be_bytes());
    hash.update(value.boot_epoch.to_be_bytes());
    hash.update(value.generation.to_be_bytes());
    hash.update(value.policy_epoch.to_be_bytes());
    hash.update(value.upload_handle.to_be_bytes());
    hash.update(value.profile_len.to_be_bytes());
    hash.update(value.profile_digest);
    hash.update(value.pending_spki_digest);
    hash.update((value.spki.len() as u16).to_be_bytes());
    hash.update(value.spki.as_slice());
    hash.update(value.tpm_public_digest);
    hash.finalize().into()
}

pub(crate) fn encode_header<A: ProfileBankAuthenticator>(
    value: &ProfileBankMetadata,
    auth: &A,
    output: &mut [u8; HEADER_MAX],
) -> Result<usize, WireError> {
    let body_len = put_metadata(value, output)?;
    append_tag(body_len, auth, output)
}

pub(crate) fn decode_header<A: ProfileBankAuthenticator>(
    input: &[u8],
    auth: &A,
) -> Option<ProfileBankMetadata> {
    let body = authenticated_body(input, auth)?;
    let mut reader = Reader::new(body);
    if reader.take(4).ok()? != HEADER_MAGIC || reader.u8().ok()? != VERSION {
        return None;
    }
    let slot = reader.u8().ok()?;
    let device_id = reader.array().ok()?;
    let authority_id = reader.array().ok()?;
    let authority_epoch = reader.u64().ok()?;
    let boot_epoch = reader.u64().ok()?;
    let generation = reader.u64().ok()?;
    let policy_epoch = reader.u64().ok()?;
    let upload_handle = reader.u64().ok()?;
    let profile_len = reader.u32().ok()?;
    let profile_digest = reader.array().ok()?;
    let pending_spki_digest = reader.array().ok()?;
    let spki_len = reader.u16().ok()? as usize;
    let spki = Bounded::<SPKI_MAX>::from_slice(reader.take(spki_len).ok()?)?;
    let tpm_public_digest = reader.array().ok()?;
    (reader.remaining() == 0).then_some(ProfileBankMetadata {
        slot,
        device_id,
        authority_id,
        authority_epoch,
        boot_epoch,
        generation,
        policy_epoch,
        upload_handle,
        profile_len,
        profile_digest,
        pending_spki_digest,
        spki,
        tpm_public_digest,
    })
}

pub(crate) fn encode_chunk<A: ProfileBankAuthenticator>(
    metadata: &ProfileBankMetadata,
    index: u8,
    bytes: &[u8],
    auth: &A,
    output: &mut [u8; CHUNK_RECORD_MAX],
) -> Result<usize, WireError> {
    let body_len = {
        let mut writer = Writer::new(output);
        writer.put(CHUNK_MAGIC)?;
        writer.u8(VERSION)?;
        writer.u8(metadata.slot)?;
        writer.u8(index)?;
        writer.put(&metadata_digest(metadata))?;
        writer.u16(
            bytes
                .len()
                .try_into()
                .map_err(|_| WireError::OversizePayload)?,
        )?;
        writer.put(bytes)?;
        writer.len()
    };
    append_tag(body_len, auth, output)
}

pub(crate) fn decode_chunk<'a, A: ProfileBankAuthenticator>(
    input: &'a [u8],
    auth: &A,
) -> Result<Chunk<'a>, ChunkDecodeError> {
    let body = authenticated_body(input, auth).ok_or(ChunkDecodeError::Unauthenticated)?;
    parse_chunk(body).ok_or(ChunkDecodeError::Malformed)
}

fn parse_chunk(body: &[u8]) -> Option<Chunk<'_>> {
    let mut reader = Reader::new(body);
    if reader.take(4).ok()? != CHUNK_MAGIC || reader.u8().ok()? != VERSION {
        return None;
    }
    let slot = reader.u8().ok()?;
    let index = reader.u8().ok()?;
    let metadata_digest = reader.array().ok()?;
    let length = reader.u16().ok()? as usize;
    let bytes = reader.take(length).ok()?;
    (reader.remaining() == 0).then_some(Chunk {
        slot,
        index,
        metadata_digest,
        bytes,
    })
}
fn put_metadata(value: &ProfileBankMetadata, output: &mut [u8]) -> Result<usize, WireError> {
    let mut writer = Writer::new(output);
    writer.put(HEADER_MAGIC)?;
    writer.u8(VERSION)?;
    writer.u8(value.slot)?;
    writer.put(&value.device_id)?;
    writer.put(&value.authority_id)?;
    writer.u64(value.authority_epoch)?;
    writer.u64(value.boot_epoch)?;
    writer.u64(value.generation)?;
    writer.u64(value.policy_epoch)?;
    writer.u64(value.upload_handle)?;
    writer.u32(value.profile_len)?;
    writer.put(&value.profile_digest)?;
    writer.put(&value.pending_spki_digest)?;
    writer.u16(
        value
            .spki
            .len()
            .try_into()
            .map_err(|_| WireError::OversizePayload)?,
    )?;
    writer.put(value.spki.as_slice())?;
    writer.put(&value.tpm_public_digest)?;
    Ok(writer.len())
}

fn append_tag<A: ProfileBankAuthenticator>(
    body_len: usize,
    auth: &A,
    output: &mut [u8],
) -> Result<usize, WireError> {
    let end = body_len
        .checked_add(TAG_LEN)
        .ok_or(WireError::BufferTooSmall)?;
    if end > output.len() {
        return Err(WireError::BufferTooSmall);
    }
    let tag = auth.authenticate(&output[..body_len]);
    output[body_len..end].copy_from_slice(&tag);
    Ok(end)
}

fn authenticated_body<'a, A: ProfileBankAuthenticator>(
    input: &'a [u8],
    auth: &A,
) -> Option<&'a [u8]> {
    let body_len = input.len().checked_sub(TAG_LEN)?;
    let expected = auth.authenticate(&input[..body_len]);
    constant_time_eq(&expected, &input[body_len..]).then_some(&input[..body_len])
}
