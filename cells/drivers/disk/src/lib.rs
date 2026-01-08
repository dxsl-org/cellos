#![no_std]

extern crate alloc;

use ostd::prelude::*;
use api::block::ViBlockDevice;
use alloc::vec::Vec;
use alloc::vec;
use core::cell::RefCell;

// 4MB RamDisk (Reduced from 40MB to be safer on heap)
const DISK_SIZE: usize = 4 * 1024 * 1024;
const SECTOR_SIZE: usize = 512;

pub struct RamDisk {
    data: RefCell<Vec<u8>>,
}

// SAFETY: This is a single-threaded cell environment (SAS).
// In a multi-threaded environment, this would require a Mutex.
// Since RefCell is Send but not Sync, we force Sync here.
// This means we promise not to access this from multiple threads concurrently.
unsafe impl Sync for RamDisk {}

impl RamDisk {
    pub fn new() -> Self {
        Self {
            data: RefCell::new(vec![0u8; DISK_SIZE]),
        }
    }
}

impl ViBlockDevice for RamDisk {
    fn read_sector(&self, sector: u64, buf: &mut [u8]) -> ViResult<()> {
        // Use borrow() to enforce runtime checking
        let data = self.data.borrow();

        let offset = (sector as usize) * SECTOR_SIZE;
        if offset + buf.len() > data.len() {
            return Err(ViError::InvalidInput);
        }
        if buf.len() != SECTOR_SIZE {
             return Err(ViError::InvalidInput);
        }
        buf.copy_from_slice(&data[offset..offset + buf.len()]);
        Ok(())
    }

    fn write_sector(&self, sector: u64, buf: &[u8]) -> ViResult<()> {
        // Use borrow_mut() to enforce runtime checking
        let mut data = self.data.borrow_mut();

        let offset = (sector as usize) * SECTOR_SIZE;
        if offset + buf.len() > data.len() {
            return Err(ViError::InvalidInput);
        }
        if buf.len() != SECTOR_SIZE {
            return Err(ViError::InvalidInput);
       }

        data[offset..offset + buf.len()].copy_from_slice(buf);
        Ok(())
    }

    fn sector_count(&self) -> u64 {
        (DISK_SIZE / SECTOR_SIZE) as u64
    }

    fn sector_size(&self) -> usize {
        SECTOR_SIZE
    }

    fn flush(&self) -> ViResult<()> {
        Ok(())
    }
}
