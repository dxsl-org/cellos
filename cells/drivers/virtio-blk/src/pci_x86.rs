//! x86_64 virtio-over-PCI discovery for the virtio-blk Driver Cell.
//!
//! Claims the first virtio-blk function offered by `FindPcieDeviceByVendor`
//! (modern ID first, then transitional), builds a [`VirtioPciTransport`] from
//! its `QueryPcieFunction` record and brings the device up. Interrupt routing
//! (phase 04b) is deferred: the transport parks every vector at `NO_VECTOR`
//! and requests poll, so this path never calls `sys_wait_irq`.
//!
//! Every refusal prints one named `[virtio-blk] refused <bdf>: <reason>` line
//! and yields `None`; the caller exits 0 so boot continues without this route.

use crate::bars_x86::BarRegions;
use crate::device::{BlkDevice, CellBlk};
use crate::hal_x86;
use abi::pcie_function::FLAG_DMA_REMAPPED;
use core::fmt;
use ostd::io::print_fmt;
use ostd::syscall::{
    sys_find_pcie_device_by_vendor, sys_query_pcie_function, BlockDevice, PcieDeviceInfo,
};
use virtio_pci_transport::{AccessPlatform, TransportError, VirtioPciTransport, VIRTIO_VENDOR_ID};

/// Transport driving the device on this architecture.
pub(crate) type CellTransport = VirtioPciTransport;

/// Device IDs in query order: modern virtio-blk, then transitional.
const BLK_DEVICE_IDS: [u16; 2] = [0x1042, 0x1001];

/// Discovery rounds (one yield apart) before concluding no function exists;
/// the same bound the other PCIe driver cells use.
const FIND_ATTEMPTS: usize = 200;

/// PCI requester ID `bus<<8 | dev<<3 | fn`, displayed as `bb:dd.f`.
pub(crate) struct Bdf(pub(crate) u32);

impl fmt::Display for Bdf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bdf = self.0;
        write!(
            f,
            "{:02x}:{:02x}.{:x}",
            (bdf >> 8) & 0xFF,
            (bdf >> 3) & 0x1F,
            bdf & 0x7
        )
    }
}

fn refuse(bdf: u32, reason: fmt::Arguments<'_>) {
    let _ = print_fmt(format_args!(
        "[virtio-blk] refused {}: {}\n",
        Bdf(bdf),
        reason
    ));
}

/// Claims the first virtio-blk function that is unowned or already ours.
fn claim() -> Option<PcieDeviceInfo> {
    let mut info = PcieDeviceInfo::zeroed();
    for _ in 0..FIND_ATTEMPTS {
        for &device_id in BLK_DEVICE_IDS.iter() {
            if let Ok(true) = sys_find_pcie_device_by_vendor(VIRTIO_VENDOR_ID, device_id, &mut info)
            {
                return Some(info);
            }
        }
        ostd::task::yield_now();
    }
    None
}

/// Discovers, claims and initialises a virtio-blk PCI function.
///
/// Returns `None` after logging why when no function exists or the claimed
/// one is refused; the device is reset (transport drop) before returning.
pub fn find_and_init_blk() -> Option<BlkDevice> {
    let Some(info) = claim() else {
        ostd::io::println("[virtio-blk] no virtio-blk PCI function present — exiting");
        return None;
    };
    let bdf = info.bdf;
    let _ = print_fmt(format_args!(
        "[virtio-blk] bdf={} irq=polled reason=04b-deferred\n",
        Bdf(bdf)
    ));

    let record = match sys_query_pcie_function(bdf) {
        Ok(record) => record,
        Err(error) => {
            refuse(bdf, format_args!("QueryPcieFunction failed: {error:?}"));
            return None;
        }
    };

    // Behind DMA remapping, a device that bypasses the platform translation
    // (no VIRTIO_F_ACCESS_PLATFORM) would DMA to untranslated addresses.
    let policy = if record.flags & FLAG_DMA_REMAPPED != 0 {
        AccessPlatform::Required
    } else {
        AccessPlatform::Accept
    };
    let mut bars = BarRegions::new();
    let transport = match VirtioPciTransport::new(&record, &mut bars, policy) {
        Ok(transport) => transport,
        Err(TransportError::AccessPlatformMissing) => {
            refuse(
                bdf,
                format_args!("VIRTIO_F_ACCESS_PLATFORM required (DMA remapped)"),
            );
            return None;
        }
        Err(error) => {
            refuse(bdf, format_args!("{error}"));
            return None;
        }
    };

    // Authorise the bounce pool only once the transport accepted the device,
    // so a refused function never gains bus mastering.
    if let Err(error) = hal_x86::prepare(bdf) {
        refuse(bdf, format_args!("DMA authorisation failed: {error:?}"));
        return None;
    }

    let blk = match CellBlk::new(transport) {
        Ok(blk) => blk,
        Err(error) => {
            refuse(bdf, format_args!("device init failed: {error}"));
            return None;
        }
    };
    bars.locate_status(&record);
    if let (false, status) = bars.is_live() {
        refuse(
            bdf,
            format_args!("device not live after init (status={status:x?})"),
        );
        return None;
    }

    let _ = print_fmt(format_args!(
        "[virtio-blk] pci {} {:04x}:{:04x} capacity={}\n",
        Bdf(bdf),
        record.vendor_id,
        record.device_id,
        blk.capacity()
    ));
    Some(BlkDevice {
        blk,
        route: BlockDevice::Pci { bdf: bdf as u16 },
        bars,
        poisoned: false,
    })
}
