//! Bitmap-based free-block allocator.

use crate::disk::{BlockDevice, FsError};
use crate::format::BLOCK_SIZE;
use alloc::vec::Vec;

pub struct BitmapAllocator {
    start_block: u64,
    bitmap_blocks: u32,
    total_blocks: u64,
    free_blocks: u64,
    data: Vec<u8>,
    dirty: bool,
}

impl BitmapAllocator {
    pub fn new(start_block: u64, bitmap_blocks: u32, total_blocks: u64) -> Self {
        let byte_len = (bitmap_blocks as usize) * BLOCK_SIZE;
        let mut data = alloc::vec![0u8; byte_len];

        // Mark blocks before FIRST_USABLE_BLOCK and bitmap blocks as allocated (1)
        let reserved_blocks = start_block + bitmap_blocks as u64;
        for b in 0..reserved_blocks {
            let byte_idx = (b / 8) as usize;
            let bit_idx = (b % 8) as u8;
            if byte_idx < data.len() {
                data[byte_idx] |= 1 << bit_idx;
            }
        }

        // Also mark any blocks past total_blocks as allocated
        for b in total_blocks..(bitmap_blocks as u64 * BLOCK_SIZE as u64 * 8) {
            let byte_idx = (b / 8) as usize;
            let bit_idx = (b % 8) as u8;
            if byte_idx < data.len() {
                data[byte_idx] |= 1 << bit_idx;
            }
        }

        let free_blocks = total_blocks.saturating_sub(reserved_blocks);

        Self {
            start_block,
            bitmap_blocks,
            total_blocks,
            free_blocks,
            data,
            dirty: true,
        }
    }

    pub fn load_from_disk<D: BlockDevice>(
        disk: &mut D,
        start_block: u64,
        bitmap_blocks: u32,
        total_blocks: u64,
    ) -> Result<Self, FsError> {
        let byte_len = (bitmap_blocks as usize) * BLOCK_SIZE;
        let mut data = alloc::vec![0u8; byte_len];

        for i in 0..bitmap_blocks {
            let mut block_buf = [0u8; BLOCK_SIZE];
            disk.read_block(start_block + i as u64, &mut block_buf)?;
            let offset = (i as usize) * BLOCK_SIZE;
            data[offset..offset + BLOCK_SIZE].copy_from_slice(&block_buf);
        }

        // Count free blocks
        let mut free_count = 0u64;
        for b in 0..total_blocks {
            let byte_idx = (b / 8) as usize;
            let bit_idx = (b % 8) as u8;
            if (data[byte_idx] & (1 << bit_idx)) == 0 {
                free_count += 1;
            }
        }

        Ok(Self {
            start_block,
            bitmap_blocks,
            total_blocks,
            free_blocks: free_count,
            data,
            dirty: false,
        })
    }

    pub fn save_to_disk<D: BlockDevice>(&mut self, disk: &mut D) -> Result<(), FsError> {
        if !self.dirty {
            return Ok(());
        }
        for i in 0..self.bitmap_blocks {
            let offset = (i as usize) * BLOCK_SIZE;
            let mut block_buf = [0u8; BLOCK_SIZE];
            block_buf.copy_from_slice(&self.data[offset..offset + BLOCK_SIZE]);
            disk.write_block(self.start_block + i as u64, &block_buf)?;
        }
        self.dirty = false;
        Ok(())
    }

    pub fn allocate_block(&mut self) -> Result<u64, FsError> {
        if self.free_blocks == 0 {
            return Err(FsError::NoSpace);
        }

        for (byte_idx, byte) in self.data.iter_mut().enumerate() {
            if *byte != 0xFF {
                for bit_idx in 0..8 {
                    let mask = 1 << bit_idx;
                    if (*byte & mask) == 0 {
                        let block = (byte_idx as u64) * 8 + (bit_idx as u64);
                        if block >= self.total_blocks {
                            return Err(FsError::NoSpace);
                        }
                        *byte |= mask;
                        self.free_blocks -= 1;
                        self.dirty = true;
                        return Ok(block);
                    }
                }
            }
        }
        Err(FsError::NoSpace)
    }

    pub fn free_block(&mut self, block: u64) -> Result<(), FsError> {
        if block >= self.total_blocks {
            return Err(FsError::PartitionOverflow);
        }
        let byte_idx = (block / 8) as usize;
        let bit_idx = (block % 8) as u8;
        let mask = 1 << bit_idx;

        if (self.data[byte_idx] & mask) != 0 {
            self.data[byte_idx] &= !mask;
            self.free_blocks += 1;
            self.dirty = true;
        }
        Ok(())
    }

    pub fn free_block_count(&self) -> u64 {
        self.free_blocks
    }
}
