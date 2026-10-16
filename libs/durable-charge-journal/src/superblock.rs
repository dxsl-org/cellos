#![forbid(unsafe_code)]

use sha2::{Digest, Sha256};

pub const SUPERBLOCK_MAGIC: &[u8; 4] = b"DCOJ";
pub const SUPERBLOCK_VERSION: u16 = 1;
pub const SUPERBLOCK_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Superblock {
    pub sequence: u64,
    pub ledger_id: [u8; 32],
    pub genesis_id: [u8; 32],
    pub writer_epoch: u64,
    pub head_revision: u64,
    pub head_block: u64,
    pub head_offset: u32,
    pub head_digest: [u8; 32],
    pub checkpoint_revision: u64,
    pub checkpoint_block: u64,
    pub total_journal_blocks: u64,
}

impl Superblock {
    pub fn new(ledger_id: [u8; 32], genesis_id: [u8; 32], writer_epoch: u64, total_blocks: u64) -> Self {
        Self {
            sequence: 1,
            ledger_id,
            genesis_id,
            writer_epoch,
            head_revision: 0,
            head_block: 2, // Block 0 and 1 are superblocks A and B
            head_offset: 0,
            head_digest: [0u8; 32],
            checkpoint_revision: 0,
            checkpoint_block: 2,
            total_journal_blocks: total_blocks,
        }
    }

    pub fn encode(&self, out: &mut [u8; 4096]) {
        out.fill(0);
        out[0..4].copy_from_slice(SUPERBLOCK_MAGIC);
        out[4..6].copy_from_slice(&SUPERBLOCK_VERSION.to_le_bytes());
        out[6..8].fill(0); // reserved
        out[8..16].copy_from_slice(&self.sequence.to_le_bytes());
        out[16..48].copy_from_slice(&self.ledger_id);
        out[48..80].copy_from_slice(&self.genesis_id);
        out[80..88].copy_from_slice(&self.writer_epoch.to_le_bytes());
        out[88..96].copy_from_slice(&self.head_revision.to_le_bytes());
        out[96..104].copy_from_slice(&self.head_block.to_le_bytes());
        out[104..108].copy_from_slice(&self.head_offset.to_le_bytes());
        out[108..112].fill(0); // reserved
        out[112..144].copy_from_slice(&self.head_digest);
        out[144..152].copy_from_slice(&self.checkpoint_revision.to_le_bytes());
        out[152..160].copy_from_slice(&self.checkpoint_block.to_le_bytes());
        out[160..168].copy_from_slice(&self.total_journal_blocks.to_le_bytes());

        // Checksum over bytes 0..224
        let mut hasher = Sha256::new();
        hasher.update(b"cellos/durable-charge/superblock/v1\0");
        hasher.update(&out[0..224]);
        out[224..256].copy_from_slice(&hasher.finalize());
    }

    pub fn decode(buf: &[u8; 4096]) -> Result<Self, crate::JournalError> {
        if &buf[0..4] != SUPERBLOCK_MAGIC {
            return Err(crate::JournalError::Magic);
        }
        let version = u16::from_le_bytes(buf[4..6].try_into().unwrap());
        if version != SUPERBLOCK_VERSION {
            return Err(crate::JournalError::Version);
        }

        // Verify checksum
        let mut hasher = Sha256::new();
        hasher.update(b"cellos/durable-charge/superblock/v1\0");
        hasher.update(&buf[0..224]);
        let expected_checksum = hasher.finalize();
        if &buf[224..256] != expected_checksum.as_slice() {
            return Err(crate::JournalError::Checksum);
        }

        let sequence = u64::from_le_bytes(buf[8..16].try_into().unwrap());
        let mut ledger_id = [0u8; 32];
        ledger_id.copy_from_slice(&buf[16..48]);
        let mut genesis_id = [0u8; 32];
        genesis_id.copy_from_slice(&buf[48..80]);
        let writer_epoch = u64::from_le_bytes(buf[80..88].try_into().unwrap());
        let head_revision = u64::from_le_bytes(buf[88..96].try_into().unwrap());
        let head_block = u64::from_le_bytes(buf[96..104].try_into().unwrap());
        let head_offset = u32::from_le_bytes(buf[104..108].try_into().unwrap());
        let mut head_digest = [0u8; 32];
        head_digest.copy_from_slice(&buf[112..144]);
        let checkpoint_revision = u64::from_le_bytes(buf[144..152].try_into().unwrap());
        let checkpoint_block = u64::from_le_bytes(buf[152..160].try_into().unwrap());
        let total_journal_blocks = u64::from_le_bytes(buf[160..168].try_into().unwrap());

        if head_block < 2 || head_block >= total_journal_blocks || head_offset >= 4096 {
            return Err(crate::JournalError::Corrupt);
        }

        Ok(Self {
            sequence,
            ledger_id,
            genesis_id,
            writer_epoch,
            head_revision,
            head_block,
            head_offset,
            head_digest,
            checkpoint_revision,
            checkpoint_block,
            total_journal_blocks,
        })
    }
}
