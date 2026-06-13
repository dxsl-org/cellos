//! VirtIO block device adapter for RedoxFS — maps 4 KiB RedoxFS blocks to
//! 512-byte VirtIO sectors on MBR partition P5 (`PART_SRV_BASE_LBA`).
//!
//! BLOCK_SIZE (redoxfs) = 4096 bytes = 8 × 512-byte sectors.
//! block N on the RedoxFS volume → sectors PART_SRV_BASE_LBA + N*8 on disk.

use api::disk::{PART_SRV_BASE_LBA, PART_SRV_SECTORS};
use ostd::syscall::{sys_blk_read, sys_blk_write};
use redox_syscall::error::{Error, Result, EIO};
use redoxfs::BLOCK_SIZE;

/// Sectors per one 4 KiB RedoxFS block.
const SECTORS_PER_BLOCK: u64 = BLOCK_SIZE / 512;

/// Zero-sized disk adapter — all filesystem state lives on the P5 partition.
/// A fresh instance is equivalent to any other (no in-process cache).
pub struct VicellDisk;

impl redoxfs::Disk for VicellDisk {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        let base = PART_SRV_BASE_LBA + block * SECTORS_PER_BLOCK;
        let mut tmp = [0u8; 512];
        for (i, chunk) in buffer.chunks_mut(512).enumerate() {
            if !sys_blk_read(base + i as u64, &mut tmp) {
                return Err(Error::new(EIO));
            }
            let n = chunk.len();
            chunk.copy_from_slice(&tmp[..n]);
        }
        Ok(buffer.len())
    }

    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> Result<usize> {
        let base = PART_SRV_BASE_LBA + block * SECTORS_PER_BLOCK;
        for (i, chunk) in buffer.chunks(512).enumerate() {
            let mut tmp = [0u8; 512];
            tmp[..chunk.len()].copy_from_slice(chunk);
            if !sys_blk_write(base + i as u64, &tmp) {
                return Err(Error::new(EIO));
            }
        }
        Ok(buffer.len())
    }

    fn size(&mut self) -> Result<u64> {
        Ok(PART_SRV_SECTORS * 512)
    }
}
