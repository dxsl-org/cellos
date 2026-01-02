use virtio_drivers::device::blk::VirtIOBlk;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::transport::{DeviceType, Transport};
use core::ptr::NonNull;
use crate::process::drivers::virtio_hal::VirtioHal;
use crate::sync::Spinlock;

const VIRTIO0: usize = 0x10001000;
const VIRTIO_MMIO_INTERVAL: usize = 0x1000;
const VIRTIO_MAX_DEVICES: usize = 8;

pub static BLOCK_DEVICE: Spinlock<Option<VirtIOBlk<VirtioHal, MmioTransport>>> = Spinlock::new(None);

pub fn init_driver() {
    log::info!("VirtIO: Scanning for Block Device (v0.7)...");
    for i in 0..VIRTIO_MAX_DEVICES {
        let addr = VIRTIO0 + i * VIRTIO_MMIO_INTERVAL;
        
        let header_ptr = NonNull::new(addr as *mut VirtIOHeader).unwrap();
        // Safety: Address is valid MMIO
        if let Ok(transport) = unsafe { MmioTransport::new(header_ptr) } {
             let device_type = transport.device_type();
             log::info!("VirtIO: Found device at 0x{:X} type {:?}", addr, device_type);
             
             if device_type == DeviceType::Block {
                 log::info!("VirtIO: Initializing Block Driver at 0x{:X}...", addr);
                 match VirtIOBlk::new(transport) {
                     Ok(blk) => {
                         *BLOCK_DEVICE.lock() = Some(blk);
                         log::info!("VirtIO: Block Driver initialized successfully!");
                         return;
                     }
                     Err(e) => {
                         log::error!("VirtIO: Failed to init Block Driver: {:?}", e);
                     }
                 }
             }
        }
    }
    log::warn!("VirtIO: No Block Device found.");
}

use crate::fs::{BlockDevice, Result, FsError};
use async_trait::async_trait;
use alloc::boxed::Box;

pub struct VirtIOBlockDriverWrapper;

#[async_trait]
impl BlockDevice for VirtIOBlockDriverWrapper {
    async fn read(&self, block_id: u64, buf: &mut [u8]) -> Result<()> {
        let mut guard = BLOCK_DEVICE.lock();
        if let Some(blk) = guard.as_mut() {
             // Assuming 512 bytes block size. VirtIO block operations are synchronous here.
             for (i, chunk) in buf.chunks_mut(512).enumerate() {
                 blk.read_blocks(block_id as usize + i, chunk).map_err(|_| FsError::IoError)?;
             }
             Ok(())
        } else {
            Err(FsError::NoDevice)
        }
    }

    async fn write(&self, block_id: u64, buf: &[u8]) -> Result<()> {
         let mut guard = BLOCK_DEVICE.lock();
         if let Some(blk) = guard.as_mut() {
             for (i, chunk) in buf.chunks(512).enumerate() {
                 blk.write_blocks(block_id as usize + i, chunk).map_err(|_| FsError::IoError)?;
             }
             Ok(())
         } else {
             Err(FsError::NoDevice)
         }
    }

    fn block_size(&self) -> usize {
        512 // VirtIO standard
    }

    async fn sync(&self) -> Result<()> {
        Ok(())
    }
}
