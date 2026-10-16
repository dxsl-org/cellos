#![forbid(unsafe_code)]

use cellos_fs::disk::BlockDevice;
use crate::superblock::Superblock;
use crate::JournalError;

pub struct Journal<D: BlockDevice> {
    device: D,
    active_superblock: Superblock,
    active_slot: u8, // 0 for Block 0 (A), 1 for Block 1 (B)
}

impl<D: BlockDevice> Journal<D> {
    /// Format a fresh double-buffered DCO1 journal on the given device.
    pub fn format(
        mut device: D,
        ledger_id: [u8; 32],
        genesis_id: [u8; 32],
        writer_epoch: u64,
    ) -> Result<Self, JournalError> {
        let total_blocks = device.total_blocks();
        if total_blocks < 4 {
            return Err(JournalError::Capacity);
        }

        let sb_a = Superblock::new(ledger_id, genesis_id, writer_epoch, total_blocks);
        let mut buf = [0u8; 4096];
        sb_a.encode(&mut buf);
        device.write_block(0, &buf).map_err(|_| JournalError::DeviceIo)?;

        // Slot B initialized with sequence 0 (invalid/stale)
        buf.fill(0);
        device.write_block(1, &buf).map_err(|_| JournalError::DeviceIo)?;

        device.flush().map_err(|_| JournalError::DeviceIo)?;

        Ok(Self {
            device,
            active_superblock: sb_a,
            active_slot: 0,
        })
    }

    /// Open an existing journal, choosing the valid superblock with the highest sequence.
    pub fn open(mut device: D) -> Result<Self, JournalError> {
        let mut buf_a = [0u8; 4096];
        let mut buf_b = [0u8; 4096];

        let res_a = device.read_block(0, &mut buf_a)
            .map_err(|_| JournalError::DeviceIo)
            .and_then(|_| Superblock::decode(&buf_a));
        let res_b = device.read_block(1, &mut buf_b)
            .map_err(|_| JournalError::DeviceIo)
            .and_then(|_| Superblock::decode(&buf_b));

        match (res_a, res_b) {
            (Ok(sb_a), Ok(sb_b)) => {
                if sb_a.sequence >= sb_b.sequence {
                    Ok(Self { device, active_superblock: sb_a, active_slot: 0 })
                } else {
                    Ok(Self { device, active_superblock: sb_b, active_slot: 1 })
                }
            }
            (Ok(sb_a), Err(_)) => Ok(Self { device, active_superblock: sb_a, active_slot: 0 }),
            (Err(_), Ok(sb_b)) => Ok(Self { device, active_superblock: sb_b, active_slot: 1 }),
            (Err(_), Err(_)) => Err(JournalError::Uninitialized),
        }
    }

    pub fn superblock(&self) -> &Superblock {
        &self.active_superblock
    }

    pub fn device(&mut self) -> &mut D {
        &mut self.device
    }

    /// Append a pre-encoded DCO1 record bytes to the journal with double-barrier durability.
    pub fn append_record(&mut self, record_bytes: &[u8], record_digest: [u8; 32]) -> Result<u64, JournalError> {
        if record_bytes.len() < 248 {
            return Err(JournalError::RecordLength);
        }

        let mut curr_block = self.active_superblock.head_block;
        let mut curr_offset = self.active_superblock.head_offset as usize;
        let total_blocks = self.active_superblock.total_journal_blocks;

        // Verify available capacity
        let needed_bytes = record_bytes.len();
        let remaining_in_curr = 4096 - curr_offset;
        let additional_bytes = needed_bytes.saturating_sub(remaining_in_curr);
        let additional_blocks = (additional_bytes + 4095) / 4096;
        if curr_block + (additional_blocks as u64) >= total_blocks {
            return Err(JournalError::Capacity);
        }

        // Barrier 1: write all record data blocks
        let mut written = 0;
        let mut block_buf = [0u8; 4096];

        while written < needed_bytes {
            let chunk_len = core::cmp::min(needed_bytes - written, 4096 - curr_offset);
            if curr_offset > 0 || chunk_len < 4096 {
                // Partial block write: read-modify-write
                self.device.read_block(curr_block, &mut block_buf)
                    .map_err(|_| JournalError::DeviceIo)?;
            }
            block_buf[curr_offset..curr_offset + chunk_len]
                .copy_from_slice(&record_bytes[written..written + chunk_len]);

            self.device.write_block(curr_block, &block_buf)
                .map_err(|_| JournalError::DeviceIo)?;

            written += chunk_len;
            curr_offset += chunk_len;
            if curr_offset == 4096 {
                curr_block += 1;
                curr_offset = 0;
            }
        }

        // Flush data blocks to hardware media
        self.device.flush().map_err(|_| JournalError::DeviceIo)?;

        // Barrier 2: prepare next superblock and write to the alternate slot
        let next_slot = if self.active_slot == 0 { 1 } else { 0 };
        let mut next_sb = self.active_superblock;
        next_sb.sequence = self.active_superblock.sequence.checked_add(1).ok_or(JournalError::Overflow)?;
        next_sb.head_revision = self.active_superblock.head_revision.checked_add(1).ok_or(JournalError::Overflow)?;
        next_sb.head_block = curr_block;
        next_sb.head_offset = curr_offset as u32;
        next_sb.head_digest = record_digest;

        let mut sb_buf = [0u8; 4096];
        next_sb.encode(&mut sb_buf);

        self.device.write_block(next_slot as u64, &sb_buf)
            .map_err(|_| JournalError::DeviceIo)?;

        // Flush alternate superblock
        self.device.flush().map_err(|_| JournalError::DeviceIo)?;

        self.active_superblock = next_sb;
        self.active_slot = next_slot;

        Ok(self.active_superblock.head_revision)
    }
}
