use crate::sync::Spinlock;
use crate::task::drivers::virtio_hal::VirtioHal;
// use core::ptr::NonNull;
use virtio_drivers::device::blk::VirtIOBlk;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::transport::{DeviceType, Transport};

const VIRTIO0: usize = 0x10001000;
const VIRTIO_MMIO_INTERVAL: usize = 0x1000;
const VIRTIO_MAX_DEVICES: usize = 8;

pub struct SafeVirtIOBlk(VirtIOBlk<VirtioHal, MmioTransport>);
unsafe impl Send for SafeVirtIOBlk {}
unsafe impl Sync for SafeVirtIOBlk {}

pub static BLOCK_DEVICE: Spinlock<Option<SafeVirtIOBlk>> = Spinlock::new(None);
/// IRQ number assigned to the block device during probing (slot_index + 1 for QEMU VirtIO MMIO).
static BLOCK_DEVICE_IRQ: Spinlock<u32> = Spinlock::new(0);

// Block Driver Implementation
// Helper for direct debugging
fn puts(s: &str) {
    for c in s.bytes() {
        let _ = crate::hal::sbi::console_putchar(c);
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
                            // Record which IRQ this slot maps to (QEMU: slot i → IRQ i+1).
                            *BLOCK_DEVICE_IRQ.lock() = (i as u32) + 1;
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

/// Called from the trap handler when any VirtIO MMIO IRQ fires (IRQs 1-8).
///
/// Dispatches to the matching device and calls `ack_interrupt()` to clear the
/// device's `InterruptStatus` register.  Without this call the device's IRQ line
/// stays asserted, the PLIC re-fires the interrupt immediately after `plic_complete`,
/// creating an interrupt storm that deadlocks all polling loops.
#[no_mangle]
pub extern "Rust" fn vi_handle_virtio_irq(irq: u32) {
    // --- Block device ---
    let block_irq = *BLOCK_DEVICE_IRQ.lock();
    if block_irq != 0 && block_irq == irq {
        let mut dev_lock = BLOCK_DEVICE.lock();
        if let Some(dev) = dev_lock.as_mut() {
            dev.0.ack_interrupt();
        }
        return;
    }

    // --- Input (keyboard) device ---
    // ack_irq clears InterruptStatus; without this an input IRQ becomes a storm.
    if crate::task::drivers::virtio_input::ack_irq(irq) {
        return;
    }

    // Unknown VirtIO slot — IRQ is already completed by the trap handler via plic_complete.
}

use api::block::ViBlockDevice;
use types::{ViError, ViResult};

#[allow(non_camel_case_types)]
pub struct viVirtIOBlk;

impl ViBlockDevice for viVirtIOBlk {
    fn read_sector(&self, sector: u64, buf: &mut [u8]) -> ViResult<()> {
        let mut dev_lock = BLOCK_DEVICE.lock();
        if let Some(dev) = dev_lock.as_mut() {
            // Use standard read_blocks for now (Polling/Spinning inside virtio-drivers)
            // Optimization: We could implement a custom wait here if virtio-drivers allowed it.
            // For now, at least interrupts are ENABLED and ACKED, testing the plumbing.

            match dev.0.read_blocks(sector as usize, buf) {
                Ok(_) => Ok(()),
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
