#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use api::block::ViBlockDevice;
use ostd::prelude::*;

// 4MB RamDisk (Reduced from 40MB to be safer on heap)
const DISK_SIZE: usize = 4 * 1024 * 1024;
const SECTOR_SIZE: usize = 512;

pub struct RamDisk {
    data: Mutex<Vec<u8>>,
}


impl RamDisk {
    pub fn new() -> Self {
        Self {
            data: Mutex::new(vec![0u8; DISK_SIZE]),
        }
    }
}

impl ViBlockDevice for RamDisk {
    fn read_sector(&self, sector: u64, buf: &mut [u8]) -> ViResult<()> {
        let data = self.data.lock();

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
        let mut data = self.data.lock();

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
        (DISK_SIZE / SECTOR_SIZE) as u64  // constant, no lock needed
    }

    fn sector_size(&self) -> usize {
        SECTOR_SIZE
    }

    fn flush(&self) -> ViResult<()> {
        Ok(())
    }
}
