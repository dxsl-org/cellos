#![forbid(unsafe_code)]

//! Superblock slot format v2: Ed25519-signed under the journal writer key.
//!
//! v1 sealed slots with an unkeyed SHA-256, so anyone able to write the disk could
//! roll the head back or splice another ledger's slot and recompute the "checksum".
//! v2 slots are refused unless the writer key signed
//! `SIGNING_DOMAIN || bytes[0..SIGNED_LEN]`; v1 slots fail the version check.

use crate::JournalError;
use durable_charge_codec::record::OVERHEAD;
use ed25519_compact::{PublicKey, SecretKey, Signature};

pub const SUPERBLOCK_MAGIC: &[u8; 4] = b"DCOJ";
pub const SUPERBLOCK_VERSION: u16 = 2;
pub const SUPERBLOCK_BYTES: usize = SIGNED_LEN + Signature::BYTES;
pub const BLOCK_BYTES: usize = 4096;
/// Blocks 0 and 1 are slots A and B; records start at this block.
pub const FIRST_DATA_BLOCK: u64 = 2;
const SIGNED_LEN: usize = 192;
const SIGNING_DOMAIN: &[u8] = b"cellos/durable-charge/superblock/v2\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Superblock {
    pub sequence: u64,
    pub ledger_id: [u8; 32],
    pub genesis_id: [u8; 32],
    pub writer_epoch: u64,
    pub writer_public_key: [u8; 32],
    /// Revision (DCO1 `local_revision`) of the head record; 0 = no records yet.
    pub head_revision: u64,
    /// First block of the head record (records are block-aligned); 0 when empty.
    pub head_record_block: u64,
    pub head_record_len: u32,
    /// Digest of the head record; at revision 0 it is `genesis_id`, the chain root
    /// that the first record's `previous_record_digest` must name.
    pub head_digest: [u8; 32],
    /// First free block; equals `total_journal_blocks` when the journal is full.
    pub next_block: u64,
    pub total_journal_blocks: u64,
}

impl Superblock {
    pub fn genesis(
        ledger_id: [u8; 32],
        genesis_id: [u8; 32],
        writer_epoch: u64,
        writer_public_key: [u8; 32],
        total_blocks: u64,
    ) -> Self {
        Self {
            sequence: 1,
            ledger_id,
            genesis_id,
            writer_epoch,
            writer_public_key,
            head_revision: 0,
            head_record_block: 0,
            head_record_len: 0,
            head_digest: genesis_id,
            next_block: FIRST_DATA_BLOCK,
            total_journal_blocks: total_blocks,
        }
    }

    pub fn encode(&self, signer: &SecretKey, out: &mut [u8; BLOCK_BYTES]) {
        out.fill(0);
        out[0..4].copy_from_slice(SUPERBLOCK_MAGIC);
        out[4..6].copy_from_slice(&SUPERBLOCK_VERSION.to_le_bytes());
        out[8..16].copy_from_slice(&self.sequence.to_le_bytes());
        out[16..48].copy_from_slice(&self.ledger_id);
        out[48..80].copy_from_slice(&self.genesis_id);
        out[80..88].copy_from_slice(&self.writer_epoch.to_le_bytes());
        out[88..120].copy_from_slice(&self.writer_public_key);
        out[120..128].copy_from_slice(&self.head_revision.to_le_bytes());
        out[128..136].copy_from_slice(&self.head_record_block.to_le_bytes());
        out[136..140].copy_from_slice(&self.head_record_len.to_le_bytes());
        out[144..176].copy_from_slice(&self.head_digest);
        out[176..184].copy_from_slice(&self.next_block.to_le_bytes());
        out[184..192].copy_from_slice(&self.total_journal_blocks.to_le_bytes());
        let signature = signer.sign(signed_message(out), None);
        out[SIGNED_LEN..SUPERBLOCK_BYTES].copy_from_slice(signature.as_ref());
    }

    /// Accepts only a slot signed by `writer` whose fields are self-consistent.
    /// Identity, rollback and head-record checks need context and live in `Journal::open`.
    pub fn decode(buf: &[u8; BLOCK_BYTES], writer: &[u8; 32]) -> Result<Self, JournalError> {
        if &buf[0..4] != SUPERBLOCK_MAGIC {
            return Err(JournalError::Magic);
        }
        if u16_at(buf, 4) != SUPERBLOCK_VERSION {
            return Err(JournalError::Version);
        }
        let signature = Signature::from_slice(&buf[SIGNED_LEN..SUPERBLOCK_BYTES])
            .map_err(|_| JournalError::Signature)?;
        PublicKey::new(*writer)
            .verify(signed_message(buf), &signature)
            .map_err(|_| JournalError::Signature)?;
        // Reserved bytes are part of the canonical image; nonzero means a buggy writer.
        if buf[6..8] != [0; 2]
            || buf[140..144] != [0; 4]
            || buf[SUPERBLOCK_BYTES..].iter().any(|b| *b != 0)
        {
            return Err(JournalError::Corrupt);
        }
        let sb = Self {
            sequence: u64_at(buf, 8),
            ledger_id: buf[16..48].try_into().unwrap(),
            genesis_id: buf[48..80].try_into().unwrap(),
            writer_epoch: u64_at(buf, 80),
            writer_public_key: buf[88..120].try_into().unwrap(),
            head_revision: u64_at(buf, 120),
            head_record_block: u64_at(buf, 128),
            head_record_len: u32::from_le_bytes(buf[136..140].try_into().unwrap()),
            head_digest: buf[144..176].try_into().unwrap(),
            next_block: u64_at(buf, 176),
            total_journal_blocks: u64_at(buf, 184),
        };
        if sb.writer_public_key != *writer || sb.sequence == 0 || !sb.extent_is_consistent() {
            return Err(JournalError::Corrupt);
        }
        Ok(sb)
    }

    /// The head extent must lie inside the data area and end exactly at `next_block`,
    /// so a signed slot can never point past the device (S-6) or overlap the slots.
    fn extent_is_consistent(&self) -> bool {
        if self.head_revision == 0 {
            return self.head_record_block == 0
                && self.head_record_len == 0
                && self.head_digest == self.genesis_id
                && self.next_block == FIRST_DATA_BLOCK
                && self.total_journal_blocks > FIRST_DATA_BLOCK;
        }
        if self.head_record_block < FIRST_DATA_BLOCK || (self.head_record_len as usize) < OVERHEAD {
            return false;
        }
        self.head_record_block
            .checked_add(blocks_for(self.head_record_len as usize))
            .is_some_and(|end| end == self.next_block && end <= self.total_journal_blocks)
    }
}

/// Whole blocks occupied by a block-aligned record of `len` bytes.
pub fn blocks_for(len: usize) -> u64 {
    len.div_ceil(BLOCK_BYTES) as u64
}

fn signed_message(buf: &[u8; BLOCK_BYTES]) -> [u8; SIGNING_DOMAIN.len() + SIGNED_LEN] {
    let mut message = [0u8; SIGNING_DOMAIN.len() + SIGNED_LEN];
    message[..SIGNING_DOMAIN.len()].copy_from_slice(SIGNING_DOMAIN);
    message[SIGNING_DOMAIN.len()..].copy_from_slice(&buf[..SIGNED_LEN]);
    message
}

fn u16_at(buf: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(buf[at..at + 2].try_into().unwrap())
}

fn u64_at(buf: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(buf[at..at + 8].try_into().unwrap())
}
