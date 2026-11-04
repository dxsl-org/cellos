// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Fake 434 records and BAR memory. The layout mirrors QEMU's modern virtio
//! PCI functions: one 64-bit BAR 4 holding common/ISR/device/notify at 4 KiB
//! strides with `notify_off_multiplier` 4.

use crate::{AccessPlatform, BarMapper, VirtioPciTransport};
use api::pcie_function::{
    PcieBar, PcieCap, ViPcieFunctionV1, BAR_KIND_MEM32, BAR_KIND_MEM64, PCIE_FUNCTION_BARS,
};
use core::ptr::NonNull;
use std::vec;
use std::vec::Vec;

pub const BAR: u8 = 4;
pub const BAR_BASE: u64 = 0xfe00_0000;
pub const BAR_LEN: u64 = 0x4000;
pub const COMMON: usize = 0x0000;
pub const ISR: usize = 0x1000;
pub const DEVICE: usize = 0x2000;
pub const NOTIFY: usize = 0x3000;
pub const MULTIPLIER: u32 = 4;

/// Spec offsets inside `virtio_pci_common_cfg`, written independently of
/// the crate's own constants so a layout slip there fails the tests.
pub const DEVICE_FEATURE: usize = 4;
pub const DRIVER_FEATURE_SELECT: usize = 8;
pub const DRIVER_FEATURE: usize = 12;
pub const MSIX_CONFIG: usize = 16;
pub const DEVICE_STATUS: usize = 20;
pub const QUEUE_SELECT: usize = 22;
pub const QUEUE_SIZE: usize = 24;
pub const QUEUE_MSIX_VECTOR: usize = 26;
pub const QUEUE_ENABLE: usize = 28;
pub const QUEUE_NOTIFY_OFF: usize = 30;
pub const QUEUE_DESC: usize = 32;
pub const QUEUE_DRIVER: usize = 40;
pub const QUEUE_DEVICE: usize = 48;

/// A `virtio_pci_cap` (or `virtio_pci_notify_cap` when `multiplier` is set)
/// as the kernel copies it into the record.
pub fn vcap(cfg_type: u8, bar: u8, offset: u32, length: u32, multiplier: Option<u32>) -> PcieCap {
    let len: u8 = if multiplier.is_some() { 20 } else { 16 };
    let mut bytes = [0u8; 20];
    bytes[..5].copy_from_slice(&[0x09, 0, len, cfg_type, bar]);
    bytes[8..12].copy_from_slice(&offset.to_le_bytes());
    bytes[12..16].copy_from_slice(&length.to_le_bytes());
    if let Some(m) = multiplier {
        bytes[16..20].copy_from_slice(&m.to_le_bytes());
    }
    PcieCap {
        id: 0x09,
        offset: 0x40,
        len,
        bytes,
    }
}

/// MSI-X capability first (as QEMU lays it out), then the four structures.
pub fn qemu_caps() -> [PcieCap; 5] {
    let msix = PcieCap {
        id: 0x11,
        offset: 0x98,
        len: 12,
        ..PcieCap::default()
    };
    [
        msix,
        vcap(1, BAR, COMMON as u32, 0x1000, None),
        vcap(3, BAR, ISR as u32, 0x1000, None),
        vcap(4, BAR, DEVICE as u32, 0x1000, None),
        vcap(2, BAR, NOTIFY as u32, 0x1000, Some(MULTIPLIER)),
    ]
}

/// A modern block device (0x1042) with `caps` and BAR 4 as described above.
pub fn record_with(caps: &[PcieCap]) -> ViPcieFunctionV1 {
    let mut rec = ViPcieFunctionV1 {
        vendor_id: 0x1af4,
        device_id: 0x1042,
        cap_count: caps.len() as u16,
        ..ViPcieFunctionV1::default()
    };
    rec.bars[BAR as usize] = PcieBar {
        base: BAR_BASE,
        len: BAR_LEN,
        kind: BAR_KIND_MEM64,
        bar_flags: 0,
    };
    rec.bars[2] = PcieBar {
        base: 0xfd00_0000,
        len: 0x1000,
        kind: BAR_KIND_MEM32,
        bar_flags: 0,
    };
    rec.caps[..caps.len()].copy_from_slice(caps);
    rec
}

pub fn record() -> ViPcieFunctionV1 {
    record_with(&qemu_caps())
}

/// A transport over `fake` with [`AccessPlatform::Accept`]; panics on error.
pub fn build(fake: &mut FakeBars, rec: &ViPcieFunctionV1) -> VirtioPciTransport {
    VirtioPciTransport::new(rec, fake, AccessPlatform::Accept).unwrap()
}

bitflags::bitflags! {
    /// Device-specific feature set handed to the generic `begin_init`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Features: u64 {
        const A = 1 << 0;
        const B = 1 << 1;
    }
}

/// Zeroed, 8-aligned memory per memory BAR of a record. `map` hands out the
/// buffer and logs the call; the buffers outlive any transport built from
/// them as long as the `FakeBars` is declared first.
pub struct FakeBars {
    mem: [Vec<u64>; PCIE_FUNCTION_BARS],
    pub maps: Vec<(u8, u64, u64)>,
}

impl FakeBars {
    pub fn new(record: &ViPcieFunctionV1) -> Self {
        Self {
            mem: core::array::from_fn(|i| vec![0u64; (record.bars[i].len as usize).div_ceil(8)]),
            maps: Vec::new(),
        }
    }

    fn at<T>(&self, bar: u8, off: usize) -> *mut T {
        let mem = &self.mem[bar as usize];
        assert!(
            off + size_of::<T>() <= mem.len() * 8,
            "fake access out of BAR"
        );
        // SAFETY: bounds asserted above.
        unsafe { mem.as_ptr().cast::<u8>().cast_mut().add(off).cast() }
    }

    pub fn read<T: Copy>(&self, bar: u8, off: usize) -> T {
        // SAFETY: in bounds (`at`) and aligned by every caller's offset choice.
        unsafe { self.at::<T>(bar, off).read_volatile() }
    }

    /// Address of byte `off` of BAR `bar`'s fake memory.
    pub fn addr(&self, bar: u8, off: usize) -> *mut u8 {
        self.at(bar, off)
    }

    pub fn write<T: Copy>(&mut self, bar: u8, off: usize, value: T) {
        // SAFETY: as in `read`.
        unsafe { self.at::<T>(bar, off).write_volatile(value) }
    }

    /// Reads a common-config register of the default layout.
    pub fn common<T: Copy>(&self, off: usize) -> T {
        self.read(BAR, COMMON + off)
    }
}

impl BarMapper for FakeBars {
    fn map(&mut self, bar: u8, base: u64, len: u64) -> Option<NonNull<u8>> {
        self.maps.push((bar, base, len));
        NonNull::new(self.mem[bar as usize].as_mut_ptr().cast())
    }
}
