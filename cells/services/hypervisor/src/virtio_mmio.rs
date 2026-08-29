//! virtio-mmio Version=2 register-block emulator for one device slot.
//!
//! Handshake: ACK(1)→DRIVER(2)→feature exchange→FEATURES_OK(8)→queue setup→DRIVER_OK(4).
//! VERSION_1 (bit 32 = DriverFeatures high-word bit 0) is mandatory; rejected otherwise.
use ostd::io::println;

/// Maximum queues per device.
pub const MAX_QUEUES: usize = 2;
const QUEUE_SIZE_MAX: u16 = crate::virtqueue_guard::MAX_QUEUE_SIZE as u16;
const INVALID_QUEUE: usize = MAX_QUEUES;
const STATUS_DRIVER_OK: u32 = 0x04;
const STATUS_NEEDS_RESET: u32 = 0x40;
const STATUS_FAILED: u32 = 0x80;

// VIRTIO_F_VERSION_1 sits in feature high-word bit 0 (bit 32 of the 64-bit field).
const VIRTIO_F_VERSION_1_HI: u32 = 1;

/// Per-queue GPA layout written by the driver during initialization.
#[derive(Default, Clone, Copy)]
pub struct QueueCfg {
    pub num: u16,
    pub ready: bool,
    pub desc_gpa: u64,
    pub avail_gpa: u64,
    pub used_gpa: u64,
}

impl QueueCfg {
    pub fn is_valid(&self) -> bool {
        crate::virtqueue_guard::valid_queue_config(
            self.num,
            self.desc_gpa,
            self.avail_gpa,
            self.used_gpa,
        )
    }
}

/// Device model contract.
pub trait VirtioDevice {
    fn device_id(&self) -> u32;
    fn device_features_lo(&self) -> u32 {
        0
    }
    fn device_features_hi(&self) -> u32 {
        VIRTIO_F_VERSION_1_HI
    }
    /// Guest rang QueueNotify for queue `q` with the confirmed queue config.
    fn notify(&mut self, q: usize, qcfg: &QueueCfg, vm_id: usize, vcpu_id: usize);
    fn config_read(&self, _offset: usize) -> u32 {
        0
    }
    fn reset(&mut self) {}
    #[cfg(feature = "hostile-backend-recovery")]
    fn hostile_backend_fault(&mut self, _command: u32) {}
}

/// Register state for one virtio-mmio slot.
#[derive(Default)]
pub struct VirtioMmio {
    status: u32,
    feat_sel: u32,
    drv_feat_sel: u32,
    drv_feat_lo: u32,
    drv_feat_hi: u32,
    queue_sel: usize,
    queues: [QueueCfg; MAX_QUEUES],
    pub intr_status: u32,
}

impl VirtioMmio {
    pub fn mmio_read(&self, offset: u64, dev: &dyn VirtioDevice) -> u64 {
        let q = self.queue_sel;
        match offset {
            0x000 => 0x7472_6976, // Magic "virt"
            0x004 => 2,           // Version=2 (modern)
            0x008 => dev.device_id() as u64,
            0x00c => 0xFFFF_FFFF, // VendorID
            0x010 => {
                if self.feat_sel == 0 {
                    dev.device_features_lo() as u64
                } else {
                    dev.device_features_hi() as u64
                }
            }
            0x034 if q < MAX_QUEUES => QUEUE_SIZE_MAX as u64, // QueueNumMax
            0x038 if q < MAX_QUEUES => self.queues[q].num as u64,
            0x044 if q < MAX_QUEUES && self.queues[q].ready => 1,
            0x060 => self.intr_status as u64,
            0x070 => self.status as u64,
            o if o >= 0x100 => dev.config_read((o - 0x100) as usize) as u64,
            _ => 0,
        }
    }

    pub fn mmio_write(
        &mut self,
        offset: u64,
        val: u32,
        dev: &mut dyn VirtioDevice,
        vm_id: usize,
        vcpu_id: usize,
    ) {
        let q = self.queue_sel;
        match offset {
            0x014 => self.feat_sel = val,
            0x020 => {
                if self.drv_feat_sel == 0 {
                    self.drv_feat_lo = val;
                } else {
                    self.drv_feat_hi = val;
                }
            }
            0x024 => self.drv_feat_sel = val,
            0x030 => {
                self.queue_sel = val as usize;
                if self.queue_sel >= MAX_QUEUES {
                    println("[hv-virtio-host] reject queue-select");
                }
            }
            0x038 if q < MAX_QUEUES => {
                let queue = &mut self.queues[q];
                queue.ready = false;
                queue.num = if crate::virtqueue_guard::valid_queue_size(val as usize) {
                    val as u16
                } else {
                    0
                };
            }
            0x044 => {
                if q < MAX_QUEUES && val == 0 {
                    self.queues[q].ready = false;
                } else if q < MAX_QUEUES && val == 1 && self.queues[q].is_valid() {
                    self.queues[q].ready = true;
                } else {
                    if q < MAX_QUEUES {
                        self.queues[q].ready = false;
                    }
                    println("[hv-virtio-host] reject queue-ready");
                }
            }
            0x050 => {
                let nq = val as usize;
                if self.status & STATUS_DRIVER_OK == 0
                    || self.status & (STATUS_NEEDS_RESET | STATUS_FAILED) != 0
                {
                    println("[hv-virtio-host] reject queue-notify-before-driver-ok");
                    return;
                }
                if nq >= MAX_QUEUES || !self.queues[nq].ready || !self.queues[nq].is_valid() {
                    println("[hv-virtio-host] reject queue-notify-invalid");
                    return;
                }
                let qcfg = self.queues[nq];
                dev.notify(nq, &qcfg, vm_id, vcpu_id);
                self.signal_used();
            }
            #[cfg(feature = "hostile-backend-recovery")]
            0x0fc => dev.hostile_backend_fault(val),
            0x064 => self.intr_status &= !val, // InterruptACK
            0x070 => {
                if val == 0 {
                    dev.reset();
                    *self = VirtioMmio::default();
                    println("[hv-virtio-host] reset");
                }
                if self.status & (STATUS_NEEDS_RESET | STATUS_FAILED) != 0 {
                    return;
                }
                if val & 0x8 != 0 && self.drv_feat_hi & VIRTIO_F_VERSION_1_HI == 0 {
                    // Guest did not negotiate VERSION_1; signal NEEDS_RESET.
                    self.status |= STATUS_NEEDS_RESET;
                    return;
                }
                self.status = val;
                if val & STATUS_FAILED != 0 {
                    self.status |= STATUS_NEEDS_RESET;
                }
            }
            0x080 if q < MAX_QUEUES => {
                self.queues[q].ready = false;
                set_lo(&mut self.queues[q].desc_gpa, val);
            }
            0x084 if q < MAX_QUEUES => {
                self.queues[q].ready = false;
                set_hi(&mut self.queues[q].desc_gpa, val);
            }
            0x090 if q < MAX_QUEUES => {
                self.queues[q].ready = false;
                set_lo(&mut self.queues[q].avail_gpa, val);
            }
            0x094 if q < MAX_QUEUES => {
                self.queues[q].ready = false;
                set_hi(&mut self.queues[q].avail_gpa, val);
            }
            0x0a0 if q < MAX_QUEUES => {
                self.queues[q].ready = false;
                set_lo(&mut self.queues[q].used_gpa, val);
            }
            0x0a4 if q < MAX_QUEUES => {
                self.queues[q].ready = false;
                set_hi(&mut self.queues[q].used_gpa, val);
            }
            _ => {}
        }
    }

    /// Return a copy of the queue configuration for queue `q`.
    pub fn queue_cfg(&self, q: usize) -> QueueCfg {
        if q < MAX_QUEUES {
            self.queues[q]
        } else {
            QueueCfg::default()
        }
    }
    /// Mark a used-buffer completion pending until the guest ACKs ISR bit 0.
    pub fn signal_used(&mut self) {
        self.intr_status |= 1;
    }

    #[cfg(any(target_arch = "x86_64", test))]
    pub fn interrupt_pending(&self) -> bool {
        self.intr_status != 0
    }
}

#[inline]
fn set_lo(v: &mut u64, lo: u32) {
    *v = (*v & 0xFFFF_FFFF_0000_0000) | lo as u64;
}
#[inline]
fn set_hi(v: &mut u64, hi: u32) {
    *v = (*v & 0x0000_0000_FFFF_FFFF) | ((hi as u64) << 32);
}

#[path = "virtio-mmio-address.rs"]
mod address;
pub use address::{owns, slot_and_offset};

#[cfg(test)]
#[path = "virtio-mmio-tests.rs"]
mod tests;
