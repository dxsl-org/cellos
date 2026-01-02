#![no_std]

use core::ptr::{read_volatile, write_volatile};

pub const VIRTIO_MMIO_MAGIC_VALUE: usize = 0x000;
pub const VIRTIO_MMIO_VERSION: usize = 0x004;
pub const VIRTIO_MMIO_DEVICE_ID: usize = 0x008;
pub const VIRTIO_MMIO_VENDOR_ID: usize = 0x00c;
pub const VIRTIO_MMIO_DEVICE_FEATURES: usize = 0x010;
pub const VIRTIO_MMIO_DEVICE_FEATURES_SEL: usize = 0x014;
pub const VIRTIO_MMIO_DRIVER_FEATURES: usize = 0x020;
pub const VIRTIO_MMIO_DRIVER_FEATURES_SEL: usize = 0x024;
pub const VIRTIO_MMIO_QUEUE_SEL: usize = 0x030;
pub const VIRTIO_MMIO_QUEUE_NUM_MAX: usize = 0x034;
pub const VIRTIO_MMIO_QUEUE_NUM: usize = 0x038;
pub const VIRTIO_MMIO_QUEUE_PFN: usize = 0x040;
pub const VIRTIO_MMIO_QUEUE_NOTIFY: usize = 0x050;
pub const VIRTIO_MMIO_STATUS: usize = 0x070;

pub const VIRTIO_STATUS_ACKNOWLEDGE: u32 = 1;
pub const VIRTIO_STATUS_DRIVER: u32 = 2;
pub const VIRTIO_STATUS_FAILED: u32 = 128;
pub const VIRTIO_STATUS_FEATURES_OK: u32 = 8;
pub const VIRTIO_STATUS_DRIVER_OK: u32 = 4;

pub const VRING_DESC_F_NEXT: u16 = 1;
pub const VRING_DESC_F_WRITE: u16 = 2;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct VirtqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

#[repr(C)]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; 32],
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct VirtqUsedElem {
    pub id: u32,
    pub len: u32,
}

#[repr(C)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VirtqUsedElem; 32],
}

#[repr(C, align(4096))]
pub struct Virtqueue {
    pub desc: [VirtqDesc; 32],
    pub avail: VirtqAvail,
    pub padding: [u8; 3516], // 4096 - 512 - 68 = 3516
    pub used: VirtqUsed,
}

pub struct VirtioMmio {
    pub base: usize,
}

impl VirtioMmio {
    pub unsafe fn new(base: usize) -> Self { Self { base } }

    pub fn read_reg(&self, offset: usize) -> u32 {
        unsafe { read_volatile((self.base + offset) as *const u32) }
    }

    pub fn write_reg(&self, offset: usize, val: u32) {
        unsafe { write_volatile((self.base + offset) as *mut u32, val) }
    }

    pub fn init(&self, dev_id: u32) -> bool {
        if self.read_reg(VIRTIO_MMIO_MAGIC_VALUE) != 0x74726976 { return false; }
        if self.read_reg(VIRTIO_MMIO_DEVICE_ID) != dev_id { return false; }
        // Reset Device
        self.write_reg(VIRTIO_MMIO_STATUS, 0);
        while self.read_reg(VIRTIO_MMIO_STATUS) != 0 {}
        
        // Step 1: Acknowledge
        self.write_reg(VIRTIO_MMIO_STATUS, VIRTIO_STATUS_ACKNOWLEDGE); 
        // Step 2: Driver
        self.write_reg(VIRTIO_MMIO_STATUS, VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER);
        
        // Read Features
        let _ = self.read_reg(VIRTIO_MMIO_DEVICE_FEATURES);
        self.write_reg(VIRTIO_MMIO_DRIVER_FEATURES, 0); 

        // Step 3: Features OK
        let status = VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK;
        self.write_reg(VIRTIO_MMIO_STATUS, status);
        
        // Re-read to confirm
        if (self.read_reg(VIRTIO_MMIO_STATUS) & VIRTIO_STATUS_FEATURES_OK) == 0 {
            return false;
        }

        // Set page size (Legacy) - Needed for QEMU?
        // offset 0x028 is QueueGuestPageSize. legacy only.
         self.write_reg(0x028, 4096);
        
        true
    }

    pub fn setup_queue(&self, q_idx: u32, q_addr: usize) {
        self.write_reg(VIRTIO_MMIO_QUEUE_SEL, q_idx);
        // Check if queue exists
        if self.read_reg(VIRTIO_MMIO_QUEUE_NUM_MAX) == 0 { return; }
        
        self.write_reg(VIRTIO_MMIO_QUEUE_NUM, 32);
        // Legacy PFN setup
        self.write_reg(VIRTIO_MMIO_QUEUE_PFN, (q_addr >> 12) as u32);
    }
    
    pub fn complete_init(&self) {
        // Step 4: Driver OK
        let status = self.read_reg(VIRTIO_MMIO_STATUS) | VIRTIO_STATUS_DRIVER_OK;
        self.write_reg(VIRTIO_MMIO_STATUS, status);
    }

    pub fn notify(&self, q_idx: u32) {
        self.write_reg(VIRTIO_MMIO_QUEUE_NOTIFY, q_idx);
    }
}

pub fn memory_barrier() {
    unsafe { core::arch::asm!("fence"); }
}
