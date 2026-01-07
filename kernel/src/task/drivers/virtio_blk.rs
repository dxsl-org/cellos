use virtio_drivers::device::blk::VirtIOBlk;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::transport::{DeviceType, Transport};
use core::ptr::NonNull;
use crate::task::drivers::virtio_hal::VirtioHal;
use crate::sync::Spinlock;

const VIRTIO0: usize = 0x10001000;
const VIRTIO_MMIO_INTERVAL: usize = 0x1000;
const VIRTIO_MAX_DEVICES: usize = 8;

pub struct SafeVirtIOBlk(VirtIOBlk<VirtioHal, MmioTransport>);
unsafe impl Send for SafeVirtIOBlk {}
unsafe impl Sync for SafeVirtIOBlk {}

pub static BLOCK_DEVICE: Spinlock<Option<SafeVirtIOBlk>> = Spinlock::new(None);

// Block Driver Implementation
// Helper for direct debugging
fn puts(s: &str) {
    for c in s.bytes() {
        crate::hal::sbi::console_putchar(c);
    }
}

pub fn init_driver() {
    puts("[DEBUG] VirtIO Block: Probing start...\n");
    
    // We scan standard VirtIO MMIO slots (0x10001000 region)
    // In ViOS memory map, these are usually 0x1000_1000 onwards for 8 devices
    for i in 0..VIRTIO_MAX_DEVICES {
        let addr = VIRTIO0 + i * VIRTIO_MMIO_INTERVAL;
        let header = unsafe { core::ptr::NonNull::new_unchecked(addr as *mut VirtIOHeader) };
        
        // Safety: We assume identity mapping for MMIO regions in this phase
        match unsafe { MmioTransport::new(header) } {
            Ok(transport) => {
                if transport.device_type() == DeviceType::Block {
                    puts("[DEBUG] VirtIO Block: Found Block Device. Calling new()...\n");

                    match VirtIOBlk::<VirtioHal, MmioTransport>::new(transport) {
                        Ok(blk) => {
                            puts("[DEBUG] VirtIO Block: new() returned Ok. Locking global...\n");
                            let mut locked_dev = BLOCK_DEVICE.lock();
                            *locked_dev = Some(SafeVirtIOBlk(blk));
                            puts("[DEBUG] VirtIO Block: Driver initialized successfully!\n");
                            return; // Only support 1 block device for now
                        }
                        Err(_) => {
                            puts("[DEBUG] VirtIO Block: new() returned Error\n");
                        }
                    }
                }
            }
            Err(_) => {
                // Ignore invalid devices
            }
        }
    }
    puts("[WARN] VirtIO Block: No device found.\n");
}

use api::block::ViBlockDevice;
use types::{ViResult, ViError};

#[allow(non_camel_case_types)]
pub struct viVirtIOBlk;

impl ViBlockDevice for viVirtIOBlk {
    fn read_sector(&self, sector: u64, buf: &mut [u8]) -> ViResult<()> {
        let mut dev_lock = BLOCK_DEVICE.lock();
        if let Some(dev) = dev_lock.as_mut() {
             match dev.0.read_blocks(sector as usize, buf) {
                 Ok(_) => {
                     // Debug: Log Sector 0
                     if sector == 0 {
                         puts("VirtIO Block: Read Sector 0 OK [PUTS]\n");
                         // Hex dump
                         if buf.len() >= 2 { 
                             puts("First 16 bytes: ");
                             for i in 0..16 {
                                 let b = buf[i];
                                 let high = (b >> 4) & 0xF;
                                 let low = b & 0xF;
                                 crate::hal::sbi::console_putchar(if high < 10 { high + 48 } else { high + 55 });
                                 crate::hal::sbi::console_putchar(if low < 10 { low + 48 } else { low + 55 });
                                 crate::hal::sbi::console_putchar(32); 
                             }
                             puts("\n");
                         }
                     }
                     Ok(())
                 },
                 Err(e) => {
                     log::error!("VirtIO Block Read Error: {:?}", e);
                     Err(ViError::NotFound)
                 }
             }
        } else {
            Err(ViError::NotFound)
        }
    }

    fn write_sector(&self, sector: u64, buf: &[u8]) -> ViResult<()> {
        let mut dev_lock = BLOCK_DEVICE.lock();
        if let Some(dev) = dev_lock.as_mut() {
             match dev.0.write_blocks(sector as usize, buf) {
                 Ok(_) => Ok(()),
                 Err(e) => {
                     log::error!("VirtIO Block Write Error: {:?}", e);
                     Err(ViError::NotFound)
                 }
             }
        } else {
            Err(ViError::NotFound)
        }
    }

    fn sector_count(&self) -> u64 {
        let mut dev_lock = BLOCK_DEVICE.lock();
        if let Some(dev) = dev_lock.as_mut() {
             dev.0.capacity()
        } else {
            0
        }
    }

    fn sector_size(&self) -> usize {
        512 // VirtIO standard usually
    }
    
    fn flush(&self) -> ViResult<()> {
        Ok(())
    }
}
