#![no_std]

//! Disk Driver Cell - INTERFACE ONLY

use ostd::prelude::*;
use api::block::ViBlockDevice;

pub struct DiskDriver;

impl ViBlockDevice for DiskDriver {
    fn read_sector(&self, _sector: u64, _buf: &mut [u8]) -> Result<()> { todo!() }
    fn write_sector(&self, _sector: u64, _buf: &[u8]) -> Result<()> { todo!() }
    fn sector_count(&self) -> u64 { todo!() }
    fn sector_size(&self) -> usize { todo!() }
    fn flush(&self) -> Result<()> { todo!() }
}
