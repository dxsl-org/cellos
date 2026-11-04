//! Block device layer of the virtio-blk Driver Cell, shared by both transports.
//!
//! The transport is chosen per architecture: virtio-MMIO slots on riscv64 /
//! aarch64 ([`crate::mmio`]) and virtio-over-PCI on x86_64
//! ([`crate::pci_x86`]). `dispatch` drives either through [`BlkDevice`].
//! riscv64 / aarch64 issue blocking requests (below); x86_64 issues polled,
//! deadline-bounded ones (`crate::io_x86`). Both classify results with
//! [`outcome`].

#[cfg(not(target_arch = "x86_64"))]
pub use crate::mmio::find_and_init_blk;
#[cfg(not(target_arch = "x86_64"))]
use crate::mmio::{CellHal, CellTransport};

#[cfg(target_arch = "x86_64")]
use crate::hal_x86::CellHal;
#[cfg(target_arch = "x86_64")]
pub use crate::pci_x86::find_and_init_blk;
#[cfg(target_arch = "x86_64")]
use crate::pci_x86::CellTransport;

use ostd::syscall::BlockDevice;
use types::block_wire::BlockOutcome;
use virtio_drivers::{device::blk::VirtIOBlk, Error as VirtioError};

pub(crate) type CellBlk = VirtIOBlk<CellHal, CellTransport>;

/// Runtime state for the active VirtIO block device.
///
/// Fields drop in declaration order: `blk` (whose transport resets the device
/// and so stops its DMA) goes before the BAR mappings that transport uses.
pub struct BlkDevice {
    pub(crate) blk: CellBlk,
    /// Block route naming the device this cell claimed.
    pub(crate) route: BlockDevice,
    /// BAR mappings the PCI transport addresses; alive as long as `blk`.
    #[cfg(target_arch = "x86_64")]
    pub(crate) bars: crate::bars_x86::BarRegions,
    /// Set when a request stalled; the device is never touched again.
    #[cfg(target_arch = "x86_64")]
    pub(crate) poisoned: bool,
}

/// Classify a virtio-drivers request result by issue certainty.
///
/// Only `QueueFull` and `DmaError` are raised strictly before descriptors are
/// published and the device notified; every other error (device
/// `IOERR`/`UNSUPP` status, a used-ring mismatch) arrives after the notify, so
/// the device may already have acted on the request.
pub(crate) fn outcome(result: Result<(), VirtioError>) -> Result<(), BlockOutcome> {
    match result {
        Ok(()) => Ok(()),
        Err(VirtioError::QueueFull | VirtioError::DmaError) => Err(BlockOutcome::NotIssued),
        Err(_) => Err(BlockOutcome::Indeterminate),
    }
}

impl BlkDevice {
    /// Block route under which this device is published.
    pub fn route(&self) -> BlockDevice {
        self.route
    }
}

#[cfg(not(target_arch = "x86_64"))]
impl BlkDevice {
    /// Read one 512-byte sector.
    pub fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockOutcome> {
        outcome(self.blk.read_blocks(sector as usize, buf))
    }

    /// Write one 512-byte sector.
    pub fn write_sector(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockOutcome> {
        outcome(self.blk.write_blocks(sector as usize, buf))
    }

    /// Send a VirtIO FLUSH request.
    pub fn flush(&mut self) -> Result<(), BlockOutcome> {
        outcome(self.blk.flush())
    }
}
