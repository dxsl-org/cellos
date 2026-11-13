//! Controller discovery and virtio bring-up up to `DRIVER_OK`.

use crate::bars::Bars;
use crate::config::{self, ConfigError, Limits};
use crate::dma::{DmaArea, DmaError};
use crate::queue::{RequestQueue, REQUEST_QUEUE};
use crate::ring;
use abi::pcie_function::{ViPcieFunctionV1, FLAG_DMA_REMAPPED};
use core::fmt;
use ostd::syscall::{sys_find_pcie_device_by_vendor, PcieDeviceInfo};
use virtio_drivers::transport::Transport;
use virtio_pci_transport::{
    AccessPlatform, InitFailure, TransportError, VirtioPciTransport, VIRTIO_VENDOR_ID,
};

/// virtio-scsi PCI device IDs: modern (0x1040 + type 8), then transitional.
const DEVICE_IDS: [u16; 2] = [0x1048, 0x1004];
/// Discovery attempts. The cell may start while the Platform Cell's ECAM
/// scan is still registering functions, so absence is concluded only after
/// this many yields.
const DISCOVERY_ATTEMPTS: usize = 200;

bitflags::bitflags! {
    /// Device feature bits the cell accepts: none. The transport itself adds
    /// `VERSION_1` and, when offered, `ACCESS_PLATFORM`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct ScsiFeatures: u64 {}
}

/// Why bring-up stopped.
#[derive(Debug)]
pub enum InitError {
    Transport(TransportError),
    /// The device rejected feature negotiation.
    Negotiation(Option<InitFailure>),
    ConfigSpace(virtio_drivers::Error),
    Config(ConfigError),
    /// The request queue is absent or too short for one command chain.
    QueueSize(u32),
    Dma(DmaError),
    QueueSetup(InitFailure),
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "transport: {error}"),
            Self::Negotiation(Some(failure)) => write!(f, "feature negotiation: {failure}"),
            Self::Negotiation(None) => write!(f, "feature negotiation: FEATURES_OK not kept"),
            Self::ConfigSpace(error) => write!(f, "device config: {error}"),
            Self::Config(error) => write!(f, "device config: {error:?}"),
            Self::QueueSize(max) => write!(f, "request queue max size {max} too small"),
            Self::Dma(error) => write!(f, "{error}"),
            Self::QueueSetup(failure) => write!(f, "request queue setup: {failure}"),
        }
    }
}

/// The first virtio-scsi controller this cell may claim, retried while the
/// PCI scan may still be running.
pub fn discover() -> Option<PcieDeviceInfo> {
    let mut info = PcieDeviceInfo::zeroed();
    for _ in 0..DISCOVERY_ATTEMPTS {
        for device_id in DEVICE_IDS {
            if let Ok(true) = sys_find_pcie_device_by_vendor(VIRTIO_VENDOR_ID, device_id, &mut info)
            {
                return Some(info);
            }
        }
        ostd::task::yield_now();
    }
    None
}

/// Brings the function `record` describes to `DRIVER_OK` with the request
/// queue live. Under DMA remapping the device must offer
/// `VIRTIO_F_ACCESS_PLATFORM` (`TransportError::AccessPlatformMissing`
/// otherwise), since its DMA addresses are IOVAs.
pub fn bring_up(record: &ViPcieFunctionV1) -> Result<(RequestQueue, Limits), InitError> {
    let policy = if record.flags & FLAG_DMA_REMAPPED != 0 {
        AccessPlatform::Required
    } else {
        AccessPlatform::Accept
    };
    let mut bars = Bars::new();
    let mut transport =
        VirtioPciTransport::new(record, &mut bars, policy).map_err(InitError::Transport)?;
    transport.begin_init(ScsiFeatures::empty());
    if !transport.negotiated_ok() {
        return Err(InitError::Negotiation(transport.init_failure()));
    }
    let raw = config::read(&transport).map_err(InitError::ConfigSpace)?;
    let limits = config::limits(&raw).map_err(InitError::Config)?;
    let max = transport.max_queue_size(REQUEST_QUEUE);
    let size = ring::queue_size(max).ok_or(InitError::QueueSize(max))?;
    let dma = DmaArea::new(record.bdf).map_err(InitError::Dma)?;
    let queue =
        RequestQueue::start(transport, bars, dma, size, limits).map_err(InitError::QueueSetup)?;
    Ok((queue, limits))
}
