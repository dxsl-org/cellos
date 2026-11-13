//! BAR windows of the claimed virtio PCI function and the device-status
//! read-back the x86_64 path relies on.
//!
//! The transport keeps raw pointers into these windows, so a [`BarRegions`]
//! must outlive the transport built from it. Once the device constructor owns
//! the transport, its failure record is no longer reachable; the cell reads
//! `device_status` through the common configuration structure instead.

use abi::pcie_function::{ViPcieFunctionV1, PCIE_FUNCTION_BARS};
use core::ptr::NonNull;
use ostd::mmio::{request_region, MmioRegion};
use virtio_pci_transport::{caps::parse_caps, BarMapper};

/// `device_status` byte offset in `virtio_pci_common_cfg` (virtio 1.x §4.1.4.3).
const COMMON_DEVICE_STATUS: usize = 20;
const STATUS_DRIVER_OK: u8 = 0x04;
const STATUS_NEEDS_RESET: u8 = 0x40;
const STATUS_FAILED: u8 = 0x80;

/// Every BAR the transport mapped, claimed through `request_region`.
pub struct BarRegions {
    regions: [Option<MmioRegion>; PCIE_FUNCTION_BARS],
    /// `(bar, byte offset)` of `device_status`; set by [`BarRegions::locate_status`].
    status_at: Option<(usize, usize)>,
}

impl BarMapper for BarRegions {
    fn map(&mut self, bar: u8, base: u64, len: u64) -> Option<NonNull<u8>> {
        let slot = self.regions.get_mut(usize::from(bar))?;
        let region =
            request_region(usize::try_from(base).ok()?, usize::try_from(len).ok()?).ok()?;
        // SAS: the claimed window is mapped at its physical base.
        let start = NonNull::new(region.base() as *mut u8)?;
        *slot = Some(region);
        Some(start)
    }
}

impl BarRegions {
    pub fn new() -> Self {
        Self {
            regions: core::array::from_fn(|_| None),
            status_at: None,
        }
    }

    /// Records where `device_status` lives. Call after the transport mapped
    /// the BARs; until then (or if `record` has no common structure) every
    /// status read fails.
    pub fn locate_status(&mut self, record: &ViPcieFunctionV1) {
        self.status_at = parse_caps(record)
            .ok()
            .map(|caps| (usize::from(caps.common.bar), caps.common.offset as usize));
    }

    fn device_status(&self) -> Option<u8> {
        let (bar, offset) = self.status_at?;
        let region = self.regions.get(bar)?.as_ref()?;
        region.read_u8(offset + COMMON_DEVICE_STATUS).ok()
    }

    /// The device finished initialisation: `DRIVER_OK` set, neither `FAILED`
    /// nor `DEVICE_NEEDS_RESET`. A failed negotiation always fails this.
    /// Returns the raw status alongside for logging.
    pub fn is_live(&self) -> (bool, Option<u8>) {
        let status = self.device_status();
        let live = status.is_some_and(|s| {
            s & STATUS_DRIVER_OK != 0 && s & (STATUS_FAILED | STATUS_NEEDS_RESET) == 0
        });
        (live, status)
    }

    /// The device reports `DEVICE_NEEDS_RESET` (it stops processing queues).
    /// An unreadable status counts as needing reset: fail closed.
    pub fn needs_reset(&self) -> bool {
        self.device_status()
            .is_none_or(|s| s & STATUS_NEEDS_RESET != 0)
    }
}
