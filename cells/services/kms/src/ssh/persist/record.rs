//! Record v2 of `ssh-a.bin` / `ssh-b.bin` (K4b CP1 section 4.5).
//!
//! Little endian, variable length, at most [`RECORD_MAX`] bytes:
//!
//! | off | size | field |
//! |---|---|---|
//! | 0 | 4 | magic `CKMS` |
//! | 4 | 1 | version `2` |
//! | 5 | 1 | slot (must equal the file read) |
//! | 6 | 1 | kind `1` = SSH identity |
//! | 7 | 1 | state: `0` tombstone, `1` key present |
//! | 8 | 8 | revision (`>= 1`, informational) |
//! | 16 | 32 | BLAKE2s-256 of the whole previous record, zeros for none |
//! | 48 | 1 | name length |
//! | 49 | 64 | name, zero padded |
//! | 113 | 1 | custody: `2` passphrase-wrapped (`1` plain is refused) |
//! | 114 | 32 | public key |
//! | 146 | 4 | KDF rounds |
//! | 150 | 1 | salt length |
//! | 151 | 64 | salt, zero padded |
//! | 215 | 2 | ciphertext length |
//! | 217 | n | wrapped private section, verbatim |
//! | 217+n | 32 | keyed BLAKE2s-256 over `[0, 217+n)` |
//!
//! The decoder checks length, then the MAC, and only then trusts any field.
//! The MAC key is the public dev constant: this is a format and corruption
//! check, not authentication (see `devroot`).

use blake2::{
    digest::{KeyInit, Mac},
    Blake2s256, Blake2sMac256, Digest,
};
use constant_time_eq::constant_time_eq;
use types::kms::{validate_ssh_key_name, KMS_SSH_KEY_NAME_MAX, KMS_SSH_PUBLIC_KEY_LEN};

use super::blob_io::{Slot, RECORD_MAX};
use super::devroot::mac_key;
use crate::ssh::parser::{KDF_ROUNDS_MAX, KDF_SALT_MAX, KDF_SALT_MIN, STORE_WRAPPED_CT_MAX};
use crate::ssh::store::{RecordView, SecretView, StoredIdentity};

pub(crate) const MAGIC: [u8; 4] = *b"CKMS";
pub(crate) const VERSION: u8 = 2;
pub(crate) const KIND_SSH: u8 = 1;
pub(crate) const STATE_TOMBSTONE: u8 = 0;
pub(crate) const STATE_PRESENT: u8 = 1;
pub(crate) const CUSTODY_WRAPPED: u8 = 2;

pub(crate) const HEADER_LEN: usize = 217;
pub(crate) const MAC_LEN: usize = 32;
/// Smallest wrapped section the K5 parser admits (`padded_min(16)`).
pub(crate) const CT_MIN: usize = 144;

const OFF_SLOT: usize = 5;
const OFF_KIND: usize = 6;
const OFF_STATE: usize = 7;
const OFF_REVISION: usize = 8;
const OFF_PREVIOUS: usize = 16;
const OFF_NAME_LEN: usize = 48;
const OFF_NAME: usize = 49;
const OFF_CUSTODY: usize = 113;
const OFF_PUBLIC: usize = 114;
const OFF_ROUNDS: usize = 146;
const OFF_SALT_LEN: usize = 150;
const OFF_SALT: usize = 151;
const OFF_CT_LEN: usize = 215;

/// Keyed MAC over a record body.
pub(crate) fn mac(body: &[u8]) -> [u8; MAC_LEN] {
    let key = mac_key();
    let mut hasher = <Blake2sMac256 as KeyInit>::new((&key).into());
    Mac::update(&mut hasher, body);
    let mut out = [0; MAC_LEN];
    out.copy_from_slice(&hasher.finalize().into_bytes());
    out
}

/// BLAKE2s-256 of a whole record including its MAC: what the successor stores
/// in `previous`.
pub(crate) fn digest(record: &[u8]) -> [u8; 32] {
    Blake2s256::digest(record).into()
}

/// A validated record, borrowing the bytes it was parsed from.
pub(crate) struct Parsed<'a> {
    pub present: bool,
    pub revision: u64,
    pub previous: [u8; 32],
    pub name: &'a [u8],
    pub public_key: [u8; KMS_SSH_PUBLIC_KEY_LEN],
    pub rounds: u32,
    pub salt: &'a [u8],
    pub ciphertext: &'a [u8],
}

/// What load classification needs from a record, without the key material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Meta {
    pub present: bool,
    pub revision: u64,
    pub previous: [u8; 32],
    pub digest: [u8; 32],
    /// Length of the record in the buffer it was read into.
    pub len: usize,
}

fn array<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.try_into().ok()
}

fn all_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

/// Strictly decode `bytes` as the record of file `expected`. `None` for any
/// deviation: length, MAC, magic, version, slot, kind, state, an unknown or
/// refused custody, a bound, a non-canonical name, or nonzero padding.
pub(crate) fn parse(expected: Slot, bytes: &[u8]) -> Option<Parsed<'_>> {
    let len = bytes.len();
    if !(HEADER_LEN + MAC_LEN..=RECORD_MAX).contains(&len) {
        return None;
    }
    let (body, tag) = bytes.split_at(len - MAC_LEN);
    if !constant_time_eq(&mac(body), tag) {
        return None;
    }
    if array::<4>(bytes, 0)? != MAGIC
        || *bytes.get(4)? != VERSION
        || *bytes.get(OFF_SLOT)? != expected.id()
        || *bytes.get(OFF_KIND)? != KIND_SSH
    {
        return None;
    }
    let state = *bytes.get(OFF_STATE)?;
    let revision = u64::from_le_bytes(array(bytes, OFF_REVISION)?);
    if revision == 0 {
        return None;
    }
    let previous = array::<32>(bytes, OFF_PREVIOUS)?;
    let name_len = usize::from(*bytes.get(OFF_NAME_LEN)?);
    let name_field = bytes.get(OFF_NAME..OFF_NAME + KMS_SSH_KEY_NAME_MAX)?;
    let custody = *bytes.get(OFF_CUSTODY)?;
    let public_key = array::<KMS_SSH_PUBLIC_KEY_LEN>(bytes, OFF_PUBLIC)?;
    let rounds = u32::from_le_bytes(array(bytes, OFF_ROUNDS)?);
    let salt_len = usize::from(*bytes.get(OFF_SALT_LEN)?);
    let salt_field = bytes.get(OFF_SALT..OFF_SALT + KDF_SALT_MAX)?;
    let ct_len = usize::from(u16::from_le_bytes(array(bytes, OFF_CT_LEN)?));
    if len != HEADER_LEN + ct_len + MAC_LEN {
        return None;
    }
    match state {
        STATE_TOMBSTONE => {
            if name_len != 0
                || !all_zero(name_field)
                || custody != 0
                || !all_zero(&public_key)
                || rounds != 0
                || salt_len != 0
                || !all_zero(salt_field)
                || ct_len != 0
            {
                return None;
            }
            Some(Parsed {
                present: false,
                revision,
                previous,
                name: &[],
                public_key,
                rounds,
                salt: &[],
                ciphertext: &[],
            })
        }
        STATE_PRESENT => {
            let name = name_field.get(..name_len)?;
            if !validate_ssh_key_name(name) || !all_zero(name_field.get(name_len..)?) {
                return None;
            }
            // Plain (1) is never stored by this slice and is refused here.
            if custody != CUSTODY_WRAPPED
                || !(1..=KDF_ROUNDS_MAX).contains(&rounds)
                || !(KDF_SALT_MIN..=KDF_SALT_MAX).contains(&salt_len)
                || !all_zero(salt_field.get(salt_len..)?)
                || ct_len % 16 != 0
                || !(CT_MIN..=STORE_WRAPPED_CT_MAX).contains(&ct_len)
            {
                return None;
            }
            Some(Parsed {
                present: true,
                revision,
                previous,
                name,
                public_key,
                rounds,
                salt: salt_field.get(..salt_len)?,
                ciphertext: bytes.get(HEADER_LEN..HEADER_LEN + ct_len)?,
            })
        }
        _ => None,
    }
}

/// Header facts of a valid record.
pub(crate) fn meta(expected: Slot, bytes: &[u8]) -> Option<Meta> {
    let parsed = parse(expected, bytes)?;
    Some(Meta {
        present: parsed.present,
        revision: parsed.revision,
        previous: parsed.previous,
        digest: digest(bytes),
        len: bytes.len(),
    })
}

/// The stored identity of a valid, present record (`None` for a tombstone or
/// an invalid record).
pub(crate) fn identity(expected: Slot, bytes: &[u8]) -> Option<StoredIdentity> {
    let parsed = parse(expected, bytes)?;
    if !parsed.present {
        return None;
    }
    StoredIdentity::wrapped(
        parsed.name,
        parsed.public_key,
        parsed.salt,
        parsed.rounds,
        parsed.ciphertext,
    )
}

fn put(out: &mut [u8], at: usize, bytes: &[u8]) -> Option<()> {
    out.get_mut(at..at.checked_add(bytes.len())?)?
        .copy_from_slice(bytes);
    Some(())
}

fn finish(slot: Slot, out: &mut [u8; RECORD_MAX], body_len: usize) -> Option<usize> {
    let tag = mac(out.get(..body_len)?);
    let total = body_len + MAC_LEN;
    put(out, body_len, &tag)?;
    // Everything written must load: refuse to emit what the decoder rejects.
    parse(slot, out.get(..total)?)?;
    Some(total)
}

fn header(
    slot: Slot,
    state: u8,
    revision: u64,
    previous: &[u8; 32],
    out: &mut [u8; RECORD_MAX],
) -> Option<()> {
    out.fill(0);
    put(out, 0, &MAGIC)?;
    put(out, 4, &[VERSION, slot.id(), KIND_SSH, state])?;
    put(out, OFF_REVISION, &revision.to_le_bytes())?;
    put(out, OFF_PREVIOUS, previous)
}

/// Encode a passphrase-wrapped record into `out`; returns its length. A `Plain`
/// view is refused (`None`): no plaintext seed ever reaches a record.
pub(crate) fn encode_present(
    slot: Slot,
    view: &RecordView<'_>,
    previous: &[u8; 32],
    out: &mut [u8; RECORD_MAX],
) -> Option<usize> {
    let SecretView::Wrapped {
        salt,
        rounds,
        ciphertext,
    } = &view.secret
    else {
        return None;
    };
    header(slot, STATE_PRESENT, view.revision, previous, out)?;
    put(out, OFF_NAME_LEN, &[u8::try_from(view.name.len()).ok()?])?;
    put(out, OFF_NAME, view.name)?;
    put(out, OFF_CUSTODY, &[CUSTODY_WRAPPED])?;
    put(out, OFF_PUBLIC, view.public_key)?;
    put(out, OFF_ROUNDS, &rounds.to_le_bytes())?;
    put(out, OFF_SALT_LEN, &[u8::try_from(salt.len()).ok()?])?;
    put(out, OFF_SALT, salt)?;
    put(
        out,
        OFF_CT_LEN,
        &u16::try_from(ciphertext.len()).ok()?.to_le_bytes(),
    )?;
    put(out, HEADER_LEN, ciphertext)?;
    finish(slot, out, HEADER_LEN.checked_add(ciphertext.len())?)
}

/// Encode a tombstone record into `out`; returns its length.
pub(crate) fn encode_tombstone(
    slot: Slot,
    revision: u64,
    previous: &[u8; 32],
    out: &mut [u8; RECORD_MAX],
) -> Option<usize> {
    header(slot, STATE_TOMBSTONE, revision, previous, out)?;
    finish(slot, out, HEADER_LEN)
}
