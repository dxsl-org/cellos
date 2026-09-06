//! Block device abstractions and bounded partition wrappers.

use crate::format::BLOCK_SIZE;
use alloc::vec::Vec;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FsError {
    IoError,
    Corrupted,
    NotFound,
    AlreadyExists,
    NoSpace,
    InvalidName,
    NotADirectory,
    IsADirectory,
    DirectoryNotEmpty,
    InvalidOffset,
    PartitionOverflow,
}

/// Abstract block device representing a raw storage medium or partition slice.
pub trait BlockDevice {
    /// Read one 4096-byte block.
    fn read_block(&mut self, block: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError>;

    /// Write one 4096-byte block.
    fn write_block(&mut self, block: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), FsError>;

    /// Flush all cached dirty blocks down to persistent hardware media.
    fn flush(&mut self) -> Result<(), FsError>;

    /// Vectorized read: read `count` contiguous 4 KiB blocks into `buf`.
    /// Default implementation delegates to sequential `read_block`.
    fn read_blocks(&mut self, start_block: u64, count: u32, buf: &mut [u8]) -> Result<(), FsError> {
        if buf.len() < (count as usize) * BLOCK_SIZE {
            return Err(FsError::IoError);
        }
        for i in 0..count {
            let mut block_buf = [0u8; BLOCK_SIZE];
            self.read_block(start_block + i as u64, &mut block_buf)?;
            let offset = (i as usize) * BLOCK_SIZE;
            buf[offset..offset + BLOCK_SIZE].copy_from_slice(&block_buf);
        }
        Ok(())
    }

    /// Vectorized write: write `count` contiguous 4 KiB blocks from `buf`.
    /// Default implementation delegates to sequential `write_block`.
    fn write_blocks(&mut self, start_block: u64, count: u32, buf: &[u8]) -> Result<(), FsError> {
        if buf.len() < (count as usize) * BLOCK_SIZE {
            return Err(FsError::IoError);
        }
        for i in 0..count {
            let offset = (i as usize) * BLOCK_SIZE;
            let mut block_buf = [0u8; BLOCK_SIZE];
            block_buf.copy_from_slice(&buf[offset..offset + BLOCK_SIZE]);
            self.write_block(start_block + i as u64, &block_buf)?;
        }
        Ok(())
    }

    /// Total capacity of this device in 4 KiB blocks.
    fn total_blocks(&self) -> u64;
}

/// Partition wrapper that enforces hard bounds and maps relative blocks to absolute device sectors.
pub struct BoundedDisk<D: BlockDevice> {
    inner: D,
    start_block: u64,
    block_count: u64,
}

impl<D: BlockDevice> BoundedDisk<D> {
    pub fn new(inner: D, start_block: u64, block_count: u64) -> Self {
        Self {
            inner,
            start_block,
            block_count,
        }
    }

    pub fn into_inner(self) -> D {
        self.inner
    }
}

impl<D: BlockDevice> BlockDevice for BoundedDisk<D> {
    fn read_block(&mut self, block: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if block >= self.block_count {
            return Err(FsError::PartitionOverflow);
        }
        self.inner.read_block(self.start_block + block, buf)
    }

    fn write_block(&mut self, block: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if block >= self.block_count {
            return Err(FsError::PartitionOverflow);
        }
        self.inner.write_block(self.start_block + block, buf)
    }

    fn flush(&mut self) -> Result<(), FsError> {
        self.inner.flush()
    }

    fn total_blocks(&self) -> u64 {
        self.block_count
    }
}

use alloc::sync::Arc;
use core::cell::RefCell;

struct MemDiskInner {
    blocks: Vec<[u8; BLOCK_SIZE]>,
    write_count: u64,
    flush_count: u64,
    power_cut_after_writes: Option<u64>,
}

/// In-memory block device for host tests and power-cut fuzzing.
/// Clones share the same underlying storage medium, modeling a persistent hardware disk.
#[derive(Clone)]
pub struct MemDisk {
    inner: Arc<RefCell<MemDiskInner>>,
}

impl MemDisk {
    pub fn new(total_blocks: usize) -> Self {
        Self {
            inner: Arc::new(RefCell::new(MemDiskInner {
                blocks: alloc::vec![[0u8; BLOCK_SIZE]; total_blocks],
                write_count: 0,
                flush_count: 0,
                power_cut_after_writes: None,
            })),
        }
    }

    pub fn set_power_cut(&mut self, writes: u64) {
        self.inner.borrow_mut().power_cut_after_writes = Some(writes);
    }

    pub fn write_count(&self) -> u64 {
        self.inner.borrow().write_count
    }

    pub fn flush_count(&self) -> u64 {
        self.inner.borrow().flush_count
    }
}

impl BlockDevice for MemDisk {
    fn read_block(&mut self, block: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        let inner = self.inner.borrow();
        let idx = block as usize;
        if idx >= inner.blocks.len() {
            return Err(FsError::PartitionOverflow);
        }
        buf.copy_from_slice(&inner.blocks[idx]);
        Ok(())
    }

    fn write_block(&mut self, block: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        let mut inner = self.inner.borrow_mut();
        if let Some(limit) = inner.power_cut_after_writes {
            if inner.write_count >= limit {
                return Err(FsError::IoError); // Simulates power loss!
            }
        }
        let idx = block as usize;
        if idx >= inner.blocks.len() {
            return Err(FsError::PartitionOverflow);
        }
        inner.blocks[idx].copy_from_slice(buf);
        inner.write_count += 1;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), FsError> {
        self.inner.borrow_mut().flush_count += 1;
        Ok(())
    }

    fn total_blocks(&self) -> u64 {
        self.inner.borrow().blocks.len() as u64
    }
}
