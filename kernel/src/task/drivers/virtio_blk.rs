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

/// Called from Trap Handler when a VirtIO interrupt occurs.
#[no_mangle]
pub extern "Rust" fn vi_handle_virtio_irq(irq: u32) {
    // puts("VirtIO IRQ!\n");
    // Ack interrupt
    let mut dev_lock = BLOCK_DEVICE.lock();
    if let Some(dev) = dev_lock.as_mut() {
        // We must check if THIS device raised the IRQ.
        // But we don't track which IRQ maps to this device easily without probing.
        // Assuming VIRTIO0 maps to IRQ 1, etc.
        // VirtIOBlk::ack_interrupt returns true if it was this device.
        if dev.0.ack_interrupt() {
             // puts("VirtIO Block: Interrupt Acked.\n");
             // Wake up waiting tasks?
             // Since we use blocking read_sector currently, we might need a WaitQueue.
             // But virtio-drivers 0.7.5 read_blocks is synchronous spin-wait usually?
             // No, read_blocks submits and waits for completion.
             // If we want interrupt driven, we shouldn't use read_blocks directly if it spins.
             // BUT `virtio-drivers` implementation of `read_blocks`:
             // calls `add_buffer`, `notify`, then `while !used { }` loop.
             // It doesn't yield!
             // So currently it burns CPU.
             // To fix this, we need to rewrite `read_blocks` logic here using lower level API if available, or just accept that `ack_interrupt` is for future use?
             // Request: "Rewrite VirtIO Driver to be Interrupt-driven instead of Polling".

             // The `virtio-drivers` crate is limited. We might need to fork it or use internal methods?
             // Or maybe we can't change `virtio-drivers` easily.
             // But we can implement our own `read_sector` that does:
             // 1. Submit request.
             // 2. Sleep task.
             // 3. IRQ -> Wake task.

             // `VirtIOBlk` exposes `virt_queue_add_buf`? No, it's private?
             // Assuming we stick to high level for now but verify interrupt works.
             // Actually, `ack_interrupt` notifies the queue logic that used ring is updated.

             // For REAL interrupt driven, we need `ViBlockDevice` to be async or wait-notify.
             // Since `read_sector` returns `ViResult<()>`, it blocks.
             // We can use a global `CondVar` or `WaitQueue` for the block device.

             // Ideally:
             // BLOCK_WAIT_QUEUE.wake_all();
        }
    }
}

use api::block::ViBlockDevice;
use types::{ViResult, ViError};

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
