//! Block device adapter for CellosFS Native — connects CellosFS to blk_router.
//!
//! Maps each 4 KiB CellosFS block to 8 x 512-byte sectors on the assigned MBR partition.
//! Enforces partition bounds checking to prevent partition bleed.

use crate::blk_router::{blk_flush, blk_read_sectors, blk_write_sectors};
use cellos_fs::{BlockDevice, FsError, BLOCK_SIZE};

pub struct CellosPartitionDisk {
    base_lba: u64,
    total_blocks: u64,
}

impl CellosPartitionDisk {
    pub fn new(base_lba: u64, total_sectors: u64) -> Self {
        Self {
            base_lba,
            total_blocks: total_sectors / 8,
        }
    }
}

impl BlockDevice for CellosPartitionDisk {
    fn read_block(&mut self, block: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if block >= self.total_blocks {
            return Err(FsError::PartitionOverflow);
        }
        let abs_lba = self.base_lba + block * 8;
        if blk_read_sectors(abs_lba, 8, buf) {
            Ok(())
        } else {
            Err(FsError::IoError)
        }
    }

    fn write_block(&mut self, block: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if block >= self.total_blocks {
            return Err(FsError::PartitionOverflow);
        }
        let abs_lba = self.base_lba + block * 8;
        if blk_write_sectors(abs_lba, 8, buf) {
            Ok(())
        } else {
            Err(FsError::IoError)
        }
    }

    fn flush(&mut self) -> Result<(), FsError> {
        if blk_flush() {
            Ok(())
        } else {
            Err(FsError::IoError)
        }
    }

    fn total_blocks(&self) -> u64 {
        self.total_blocks
    }
}
