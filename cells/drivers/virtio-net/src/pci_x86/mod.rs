//! x86_64 virtio-over-PCI discovery and bring-up.
//!
//! Flow: claim the function by exact vendor:device (`FindPcieDeviceByVendor`,
//! 424; modern ID first, then transitional), read its record
//! (`QueryPcieFunction`, 434), build the transport over BARs claimed with
//! `request_region` (construction resets the device), authorise DMA for the
//! BDF, then construct the driver. This order is load-bearing: the
//! ACCESS_PLATFORM policy is decided and the device is quiesced before any DMA
//! is authorised, so a refused device never gets bus mastering.
//!
//! Every refusal logs `[virtio-net] refused <bb:dd.f>: <reason>` and yields
//! `None`; the caller exits 0 so boot continues without this NIC. Interrupts
//! stay disabled at the device (MSI-X routing is deferred to phase 04b), so
//! receive is polled through the net service's Rx requests.

mod checked;
mod claims;
mod hal;
mod raw_net;
mod wait;

pub use checked::CheckedTransport;
pub use hal::PciHal;
pub use raw_net::PciNet;

use crate::device::NetDevice;
use alloc::format;
use alloc::vec::Vec;
use api::pcie_function::FLAG_DMA_REMAPPED;
use claims::BarClaims;
use core::fmt::{self, Display, Formatter};
use core::ptr::NonNull;
use ostd::io::println;
use ostd::mmio::{request_region, MmioRegion};
use ostd::syscall::{
    sys_find_pcie_device_by_vendor, sys_query_pcie_function, PcieDeviceInfo, PcieFunctionError,
};
use virtio_pci_transport::caps::parse_caps;
use virtio_pci_transport::{
    AccessPlatform, BarMapper, InitFailure, TransportError, VirtioPciTransport, VIRTIO_VENDOR_ID,
};

/// Device IDs claimed, in order: modern virtio-net, then transitional.
const NET_DEVICE_IDS: [u16; 2] = [0x1041, 0x1000];

/// `bb:dd.f` rendering of a `bus<<8 | dev<<3 | fn` requester ID.
struct Bdf(u32);

impl Display for Bdf {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let bdf = self.0;
        write!(
            f,
            "{:02x}:{:02x}.{:x}",
            (bdf >> 8) & 0xff,
            (bdf >> 3) & 0x1f,
            bdf & 0x7
        )
    }
}

/// `aa:bb:cc:dd:ee:ff` rendering of a MAC address.
struct Mac([u8; 6]);

impl Display for Mac {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{g:02x}")
    }
}

/// Why a claimed function was not brought up.
enum Refusal {
    Query(PcieFunctionError),
    Transport(TransportError),
    Dma(&'static str),
    Driver(virtio_drivers::Error),
    Init(InitFailure),
}

impl Display for Refusal {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Query(e) => write!(f, "QueryPcieFunction failed ({e:?})"),
            // Only produced under `AccessPlatform::Required`, i.e. DMA remapped.
            Self::Transport(TransportError::AccessPlatformMissing) => {
                write!(f, "VIRTIO_F_ACCESS_PLATFORM required (DMA remapped)")
            }
            Self::Transport(e) => write!(f, "{e}"),
            Self::Dma(reason) => write!(f, "{reason}"),
            Self::Driver(e) => write!(f, "virtio-net init failed ({e})"),
            Self::Init(e) => write!(f, "{e}"),
        }
    }
}

/// Claims each BAR the transport asks for, recording its index.
struct RegionMapper {
    regions: Vec<(u8, MmioRegion)>,
}

impl BarMapper for RegionMapper {
    fn map(&mut self, bar: u8, base: u64, len: u64) -> Option<NonNull<u8>> {
        let base = usize::try_from(base).ok()?;
        let len = usize::try_from(len).ok()?;
        let region = request_region(base, len).ok()?;
        self.regions.push((bar, region));
        // SAS: the claimed region's virtual address equals the BAR's physical base.
        NonNull::new(base as *mut u8)
    }
}

/// Claims the first virtio-net PCI function the kernel offers this cell.
fn discover() -> Option<PcieDeviceInfo> {
    for device_id in NET_DEVICE_IDS {
        let mut info = PcieDeviceInfo::zeroed();
        match sys_find_pcie_device_by_vendor(VIRTIO_VENDOR_ID, device_id, &mut info) {
            Ok(true) => return Some(info),
            Ok(false) => {}
            Err(_) => {
                println("[virtio-net] FindPcieDeviceByVendor denied");
                return None;
            }
        }
    }
    None
}

/// Builds the transport and driver for the claimed function `bdf`.
fn bring_up(bdf: u32) -> Result<NetDevice, Refusal> {
    let record = sys_query_pcie_function(bdf).map_err(Refusal::Query)?;
    let policy = if record.flags & FLAG_DMA_REMAPPED != 0 {
        AccessPlatform::Required
    } else {
        AccessPlatform::Accept
    };
    let mut mapper = RegionMapper {
        regions: Vec::new(),
    };
    let transport =
        VirtioPciTransport::new(&record, &mut mapper, policy).map_err(Refusal::Transport)?;
    // Already validated by `VirtioPciTransport::new`; read again for the
    // common configuration window the status probe uses.
    let caps = parse_caps(&record).map_err(Refusal::Transport)?;
    let claims = BarClaims::new(mapper.regions, caps.common);
    hal::init(bdf).map_err(Refusal::Dma)?;
    let net =
        PciNet::new(CheckedTransport::new(transport), claims, bdf).map_err(Refusal::Driver)?;
    if let Some(failure) = checked::take_failure() {
        // Dropping `net` resets the device before the refusal is reported.
        return Err(Refusal::Init(failure));
    }
    println(&format!(
        "[virtio-net] bdf={} irq=polled reason=04b-deferred",
        Bdf(bdf)
    ));
    println(&format!(
        "[virtio-net] pci {} {:04x}:{:04x} mac={}",
        Bdf(bdf),
        record.vendor_id,
        record.device_id,
        Mac(net.mac_address())
    ));
    Ok(NetDevice { net })
}

/// Discovers and initialises the virtio-net PCI function.
///
/// Returns `None` when no function is offered or bring-up refused it (logged).
pub fn find_and_init_net() -> Option<NetDevice> {
    let bdf = discover()?.bdf;
    bring_up(bdf)
        .map_err(|refusal| println(&format!("[virtio-net] refused {}: {refusal}", Bdf(bdf))))
        .ok()
}
