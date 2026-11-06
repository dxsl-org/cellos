#![forbid(unsafe_code)]

use crate::head_record::verify_head;
use crate::slot_select::{select_slot, JournalExpectation};
use crate::superblock::{blocks_for, Superblock, BLOCK_BYTES, FIRST_DATA_BLOCK};
use crate::JournalError;
use cellos_fs::disk::BlockDevice;
use durable_charge_codec::Limits;
use ed25519_compact::KeyPair;

pub struct Journal<D: BlockDevice> {
    device: D,
    active_superblock: Superblock,
    active_slot: u64, // 0 for Block 0 (A), 1 for Block 1 (B)
}

impl<D: BlockDevice> Journal<D> {
    /// Format a fresh double-buffered DCO1 journal signed by `writer`.
    pub fn format(
        mut device: D,
        ledger_id: [u8; 32],
        genesis_id: [u8; 32],
        writer_epoch: u64,
        writer: &KeyPair,
    ) -> Result<Self, JournalError> {
        // DCO1 headers refuse zero identities and epoch 0, so such a journal could never
        // accept a record; refusing here keeps the chain root (`genesis_id`) meaningful.
        if ledger_id == [0; 32] || genesis_id == [0; 32] || writer_epoch == 0 {
            return Err(JournalError::Identity);
        }
        let total_blocks = device.total_blocks();
        if total_blocks < FIRST_DATA_BLOCK + 2 {
            return Err(JournalError::Capacity);
        }
        let sb = Superblock::genesis(
            ledger_id,
            genesis_id,
            writer_epoch,
            *writer.pk,
            total_blocks,
        );
        let mut buf = [0u8; BLOCK_BYTES];
        // Clear B before A so an interrupted format never pairs a fresh A with a stale B.
        write_block(&mut device, 1, &buf)?;
        flush(&mut device)?;
        sb.encode(&writer.sk, &mut buf);
        write_block(&mut device, 0, &buf)?;
        flush(&mut device)?;
        Ok(Self {
            device,
            active_superblock: sb,
            active_slot: 0,
        })
    }

    /// Open an existing journal: see `select_slot` for slot rules. The newest slot
    /// must meet `min_sequence` and its head record must hash to the signed `head_digest`.
    pub fn open(mut device: D, expect: &JournalExpectation) -> Result<Self, JournalError> {
        let (sb, slot) = select_slot(&mut device, expect)?;
        if sb.total_journal_blocks != device.total_blocks() {
            return Err(JournalError::Corrupt);
        }
        if sb.sequence < expect.min_sequence {
            return Err(JournalError::Rollback);
        }
        verify_head(&mut device, &sb)?;
        Ok(Self {
            device,
            active_superblock: sb,
            active_slot: slot,
        })
    }

    pub fn superblock(&self) -> &Superblock {
        &self.active_superblock
    }

    pub fn device(&mut self) -> &mut D {
        &mut self.device
    }

    /// Append one DCO1 record with double-barrier durability and return its revision.
    ///
    /// The record is fully decoded here and its digest is computed from its bytes; it
    /// must belong to this ledger, carry the current writer epoch, and link to the
    /// head (`local_revision = head + 1`, `previous_record_digest = head_digest`).
    pub fn append_record(
        &mut self,
        writer: &KeyPair,
        record_bytes: &[u8],
        limits: &Limits,
    ) -> Result<u64, JournalError> {
        let sb = self.active_superblock;
        if *writer.pk != sb.writer_public_key {
            return Err(JournalError::WriterKey);
        }
        let record =
            durable_charge_codec::decode(record_bytes, limits).map_err(JournalError::Record)?;
        let header = &record.header;
        if header.journal_ledger != sb.ledger_id || header.journal_genesis != sb.genesis_id {
            return Err(JournalError::Identity);
        }
        if header.journal_writer_epoch != sb.writer_epoch {
            return Err(JournalError::StaleWriter);
        }
        let revision = sb
            .head_revision
            .checked_add(1)
            .ok_or(JournalError::Overflow)?;
        if header.local_revision != revision || header.previous_record_digest != sb.head_digest {
            return Err(JournalError::Chain);
        }
        let len = u32::try_from(record_bytes.len()).map_err(|_| JournalError::Capacity)?;
        let start = sb.next_block;
        let end = start
            .checked_add(blocks_for(record_bytes.len()))
            .ok_or(JournalError::Overflow)?;
        // `end == total` is a legal full journal; only extents past the device are refused.
        if end > sb.total_journal_blocks {
            return Err(JournalError::Capacity);
        }

        // Barrier 1: records start on a block boundary and are written as whole
        // zero-padded blocks, so no committed record block is ever read-modify-written.
        let mut block = [0u8; BLOCK_BYTES];
        for (index, chunk) in record_bytes.chunks(BLOCK_BYTES).enumerate() {
            block.fill(0);
            block[..chunk.len()].copy_from_slice(chunk);
            write_block(&mut self.device, start + index as u64, &block)?;
        }
        flush(&mut self.device)?;

        // Barrier 2: commit by signing the successor into the inactive slot.
        let next = Superblock {
            sequence: sb.sequence.checked_add(1).ok_or(JournalError::Overflow)?,
            head_revision: revision,
            head_record_block: start,
            head_record_len: len,
            head_digest: record.digest,
            next_block: end,
            ..sb
        };
        let next_slot = 1 - self.active_slot;
        next.encode(&writer.sk, &mut block);
        write_block(&mut self.device, next_slot, &block)?;
        flush(&mut self.device)?;

        self.active_superblock = next;
        self.active_slot = next_slot;
        Ok(revision)
    }
}

fn write_block<D: BlockDevice>(
    device: &mut D,
    block: u64,
    buf: &[u8; BLOCK_BYTES],
) -> Result<(), JournalError> {
    device
        .write_block(block, buf)
        .map_err(|_| JournalError::DeviceIo)
}

fn flush<D: BlockDevice>(device: &mut D) -> Result<(), JournalError> {
    device.flush().map_err(|_| JournalError::DeviceIo)
}
