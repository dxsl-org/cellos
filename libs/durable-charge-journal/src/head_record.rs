#![forbid(unsafe_code)]

//! Re-verifies the head record named by a signed superblock.
//!
//! The signature only proves the writer once committed `head_digest`; the record bytes
//! live in unsigned data blocks. Hashing them on open turns a torn or rewritten head
//! record into a refusal instead of a silently trusted chain tip.

use crate::superblock::{blocks_for, Superblock, BLOCK_BYTES};
use crate::JournalError;
use cellos_fs::disk::BlockDevice;
use durable_charge_codec::record::DIGEST_DOMAIN;
use sha2::{Digest, Sha256};

/// Streams the head extent block by block (no whole-record buffer) and requires
/// SHA-256(DIGEST_DOMAIN || body) to equal both the stored trailer and `head_digest`.
/// Since `append_record` only commits fully decoded records, digest equality also
/// re-establishes the header bindings (ledger, genesis, epoch, revision, link).
pub(crate) fn verify_head<D: BlockDevice>(
    device: &mut D,
    sb: &Superblock,
) -> Result<(), JournalError> {
    if sb.head_revision == 0 {
        return Ok(()); // Superblock::decode pinned the empty-journal extent and chain root.
    }
    let len = sb.head_record_len as usize;
    let body = len - 32; // decode guarantees len >= OVERHEAD
    let mut hasher = Sha256::new();
    hasher.update(DIGEST_DOMAIN);
    let mut trailer = [0u8; 32];
    let mut block = [0u8; BLOCK_BYTES];
    for index in 0..blocks_for(len) {
        device
            .read_block(sb.head_record_block + index, &mut block)
            .map_err(|_| JournalError::DeviceIo)?;
        let start = index as usize * BLOCK_BYTES;
        let chunk = core::cmp::min(BLOCK_BYTES, len - start);
        let hashed = core::cmp::min(chunk, body.saturating_sub(start));
        hasher.update(&block[..hashed]);
        for at in hashed..chunk {
            trailer[start + at - body] = block[at];
        }
    }
    let digest: [u8; 32] = hasher.finalize().into();
    if digest != trailer || digest != sb.head_digest {
        return Err(JournalError::HeadRecord);
    }
    Ok(())
}
