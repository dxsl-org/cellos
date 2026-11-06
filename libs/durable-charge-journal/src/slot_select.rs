#![forbid(unsafe_code)]

//! Chooses the active superblock slot on open, treating the disk as untrusted.

use crate::superblock::{Superblock, BLOCK_BYTES};
use crate::JournalError;
use cellos_fs::disk::BlockDevice;

/// What the caller already trusts about the journal before reading the disk.
/// The disk itself is untrusted: everything it returns is checked against this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalExpectation {
    pub ledger_id: [u8; 32],
    pub genesis_id: [u8; 32],
    pub writer_public_key: [u8; 32],
    /// Rollback floor. Signed slots stop forgery, not replay of an older signed slot,
    /// so callers persist `superblock().sequence` outside this device after every
    /// successful append and pass it back here.
    pub min_sequence: u64,
}

/// Returns the newest usable slot and its index. An unsigned/torn slot is ignored
/// (crash during a slot write); a validly signed slot naming another ledger is a
/// splice and fails the whole open. Floor and head checks are the caller's.
pub(crate) fn select_slot<D: BlockDevice>(
    device: &mut D,
    expect: &JournalExpectation,
) -> Result<(Superblock, u64), JournalError> {
    let a = read_slot(device, 0, expect)?;
    let b = read_slot(device, 1, expect)?;
    match (a, b) {
        (Ok(a), Ok(b)) => newer_of(a, b),
        (Ok(a), Err(_)) => Ok((a, 0)),
        (Err(_), Ok(b)) => Ok((b, 1)),
        (Err(JournalError::Magic), Err(JournalError::Magic)) => Err(JournalError::Uninitialized),
        (Err(JournalError::Magic), Err(e)) | (Err(e), Err(_)) => Err(e),
    }
}

/// Outer `Err` is fatal (I/O, splice); inner `Err` marks the slot as unusable.
fn read_slot<D: BlockDevice>(
    device: &mut D,
    slot: u64,
    expect: &JournalExpectation,
) -> Result<Result<Superblock, JournalError>, JournalError> {
    let mut buf = [0u8; BLOCK_BYTES];
    device
        .read_block(slot, &mut buf)
        .map_err(|_| JournalError::DeviceIo)?;
    let sb = match Superblock::decode(&buf, &expect.writer_public_key) {
        Ok(sb) => sb,
        Err(e) => return Ok(Err(e)),
    };
    if sb.ledger_id != expect.ledger_id || sb.genesis_id != expect.genesis_id {
        return Err(JournalError::Identity);
    }
    Ok(Ok(sb))
}

/// Both slots are writer-signed for this ledger; anything but a strictly ordered
/// predecessor/successor pair (or an identical copy) is an equivocation.
fn newer_of(a: Superblock, b: Superblock) -> Result<(Superblock, u64), JournalError> {
    if a.sequence == b.sequence {
        return if a == b {
            Ok((a, 0))
        } else {
            Err(JournalError::Corrupt)
        };
    }
    let (new, old, slot) = if a.sequence > b.sequence {
        (a, b, 0)
    } else {
        (b, a, 1)
    };
    if old.head_revision > new.head_revision || old.writer_epoch > new.writer_epoch {
        return Err(JournalError::Corrupt);
    }
    Ok((new, slot))
}
