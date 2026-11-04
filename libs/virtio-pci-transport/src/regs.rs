// SPDX-License-Identifier: Apache-2.0 OR MIT
//! `struct virtio_pci_common_cfg` register access (virtio 1.2 §4.1.4.3).
//!
//! 64-bit queue addresses are written as two 32-bit halves, low first, which
//! the spec permits and which keeps every access at most 4 bytes wide: some
//! device models only decode 32-bit accesses to this structure.

use core::ptr::NonNull;

/// Bytes of the virtio 1.0 common configuration structure the transport
/// touches; a shorter common region is refused.
pub const COMMON_CFG_LEN: usize = 56;

/// MSI-X "no vector" value: the device raises no interrupt for this source.
pub const NO_VECTOR: u16 = 0xFFFF;

/// Status reads a reset may take before the device is declared stuck. The
/// crate cannot sleep or read a clock, so the bound is an iteration count.
pub const RESET_SPIN_BUDGET: u32 = 1_000_000;

/// `device_status` as read from a function that has gone away (master abort
/// returns all ones); no valid status sets the reserved bits 4 and 5.
const STATUS_GONE: u8 = 0xFF;

/// Polls `read_status` until it returns 0 (reset complete), at most `budget`
/// times. Returns false when the budget runs out or the device reads as gone.
pub(crate) fn wait_for_reset(mut read_status: impl FnMut() -> u8, budget: u32) -> bool {
    for _ in 0..budget {
        match read_status() {
            0 => return true,
            STATUS_GONE => return false,
            _ => core::hint::spin_loop(),
        }
    }
    false
}

const DEVICE_FEATURE_SELECT: usize = 0;
const DEVICE_FEATURE: usize = 4;
const DRIVER_FEATURE_SELECT: usize = 8;
const DRIVER_FEATURE: usize = 12;
const MSIX_CONFIG: usize = 16;
const DEVICE_STATUS: usize = 20;
const QUEUE_SELECT: usize = 22;
const QUEUE_SIZE: usize = 24;
const QUEUE_MSIX_VECTOR: usize = 26;
const QUEUE_ENABLE: usize = 28;
const QUEUE_NOTIFY_OFF: usize = 30;
const QUEUE_DESC: usize = 32;
const QUEUE_DRIVER: usize = 40;
const QUEUE_DEVICE: usize = 48;
const _: () = assert!(QUEUE_DEVICE + 8 == COMMON_CFG_LEN);

/// Integer widths the common configuration structure uses.
trait Reg: Copy {}
impl Reg for u8 {}
impl Reg for u16 {}
impl Reg for u32 {}

/// Volatile view of one device's common configuration structure. Queue
/// registers apply to the queue last written to `queue_select`.
pub(crate) struct CommonRegs(NonNull<u8>);

impl CommonRegs {
    /// # Safety
    /// `base` must be 4-byte aligned and valid for volatile reads and writes
    /// of [`COMMON_CFG_LEN`] bytes for the lifetime of the returned value.
    pub(crate) unsafe fn new(base: NonNull<u8>) -> Self {
        Self(base)
    }

    fn read<T: Reg>(&self, off: usize) -> T {
        // SAFETY: `off` is a register offset inside COMMON_CFG_LEN, aligned for
        // `T` because the base is 4-aligned (contract of `new`).
        unsafe { self.0.as_ptr().add(off).cast::<T>().read_volatile() }
    }

    fn write<T: Reg>(&self, off: usize, value: T) {
        // SAFETY: as in `read`.
        unsafe { self.0.as_ptr().add(off).cast::<T>().write_volatile(value) }
    }

    fn write64(&self, off: usize, value: u64) {
        self.write(off, value as u32);
        self.write(off + 4, (value >> 32) as u32);
    }

    /// Upper device feature word (bits 32..64).
    pub(crate) fn device_features_hi(&self) -> u32 {
        self.write(DEVICE_FEATURE_SELECT, 1u32);
        self.read(DEVICE_FEATURE)
    }

    pub(crate) fn device_features(&self) -> u64 {
        self.write(DEVICE_FEATURE_SELECT, 0u32);
        let lo: u32 = self.read(DEVICE_FEATURE);
        (u64::from(self.device_features_hi()) << 32) | u64::from(lo)
    }

    pub(crate) fn set_driver_features(&self, features: u64) {
        self.write(DRIVER_FEATURE_SELECT, 0u32);
        self.write(DRIVER_FEATURE, features as u32);
        self.write(DRIVER_FEATURE_SELECT, 1u32);
        self.write(DRIVER_FEATURE, (features >> 32) as u32);
    }

    pub(crate) fn set_msix_config(&self, vector: u16) {
        self.write(MSIX_CONFIG, vector);
    }

    pub(crate) fn status(&self) -> u8 {
        self.read(DEVICE_STATUS)
    }

    pub(crate) fn set_status(&self, status: u8) {
        self.write(DEVICE_STATUS, status);
    }

    /// Writes 0 (device reset) and waits, bounded by [`RESET_SPIN_BUDGET`],
    /// for the device to report the reset complete. The spec forbids
    /// reinitialising before `device_status` reads 0. Returns false when the
    /// device never completes the reset or has gone away.
    pub(crate) fn reset(&self) -> bool {
        self.set_status(0);
        wait_for_reset(|| self.status(), RESET_SPIN_BUDGET)
    }

    pub(crate) fn select_queue(&self, queue: u16) {
        self.write(QUEUE_SELECT, queue);
    }

    pub(crate) fn queue_size(&self) -> u16 {
        self.read(QUEUE_SIZE)
    }

    pub(crate) fn set_queue_size(&self, size: u16) {
        self.write(QUEUE_SIZE, size);
    }

    pub(crate) fn set_queue_msix_vector(&self, vector: u16) {
        self.write(QUEUE_MSIX_VECTOR, vector);
    }

    pub(crate) fn queue_enable(&self) -> u16 {
        self.read(QUEUE_ENABLE)
    }

    pub(crate) fn set_queue_enable(&self, enable: u16) {
        self.write(QUEUE_ENABLE, enable);
    }

    pub(crate) fn queue_notify_off(&self) -> u16 {
        self.read(QUEUE_NOTIFY_OFF)
    }

    pub(crate) fn set_queue_areas(&self, desc: u64, driver: u64, device: u64) {
        self.write64(QUEUE_DESC, desc);
        self.write64(QUEUE_DRIVER, driver);
        self.write64(QUEUE_DEVICE, device);
    }
}
