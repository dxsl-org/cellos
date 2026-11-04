//! The BAR claims behind a PCI transport, and a read-only view of the
//! device status byte through them.
//!
//! The transport's register pointers are only valid while the cell owns these
//! claims, so they live as long as the device. Once the driver library owns
//! the transport, the status byte is read here, through the claimed common
//! configuration window, to detect `DEVICE_NEEDS_RESET`.

use alloc::vec::Vec;
use ostd::mmio::MmioRegion;
use virtio_pci_transport::caps::CapRegion;

/// `device_status` offset inside `virtio_pci_common_cfg` (virtio 1.2 §4.1.4.3).
const DEVICE_STATUS: usize = 20;

/// Every claimed BAR (by index) plus where the status byte sits in them.
pub struct BarClaims {
    regions: Vec<(u8, MmioRegion)>,
    /// `(index into regions, byte offset)` of `device_status`, when the common
    /// configuration BAR was claimed.
    status: Option<(usize, usize)>,
}

impl BarClaims {
    /// Takes ownership of `regions` (BAR index, claim); `common` is the
    /// validated common configuration structure the transport uses.
    pub fn new(regions: Vec<(u8, MmioRegion)>, common: CapRegion) -> Self {
        let status = regions
            .iter()
            .position(|(bar, _)| *bar == common.bar)
            .map(|index| (index, common.offset as usize + DEVICE_STATUS));
        Self { regions, status }
    }

    /// The current device status byte, or `None` when it cannot be read.
    pub fn device_status(&self) -> Option<u8> {
        let (index, offset) = self.status?;
        self.regions[index].1.read::<u8>(offset).ok()
    }
}
