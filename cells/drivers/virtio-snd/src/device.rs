//! Discovery, bring-up to `DRIVER_OK` (virtio 1.2 §3.1.1) and stream choice.
//! The control queue lives in [`crate::control`], playback in [`crate::tx`].

use crate::bars::Bars;
use crate::dma::DmaArea;
pub use crate::error::{CtrlError, InitError};
use crate::proto::{self, PcmInfo, EVENT_LEN, PCM_INFO_LEN, PERIODS, STATUS_LEN};
use crate::ring::{self, EVENT_BUFS};
use crate::tx::Playback;
use crate::virtq::SplitQueue;
use abi::pcie_function::{ViPcieFunctionV1, FLAG_DMA_REMAPPED};
use ostd::syscall::{sys_find_pcie_device_by_vendor, PcieDeviceInfo};
use virtio_drivers::transport::Transport;
use virtio_pci_transport::{AccessPlatform, VirtioPciTransport, VIRTIO_VENDOR_ID};

/// Modern virtio-sound PCI device ID (0x1040 + type 25); there is no
/// transitional one.
const DEVICE_ID: u16 = 0x1059;
/// Discovery attempts: the cell may start while the Platform Cell's ECAM scan
/// is still registering functions, so absence is concluded only after this
/// many yields.
const DISCOVERY_ATTEMPTS: usize = 200;
pub const CONTROLQ: u16 = 0;
pub const EVENTQ: u16 = 1;
pub const TXQ: u16 = 2;
const RXQ: u16 = 3;
/// Streams probed with PCM_INFO; the device answers a stream id past its
/// last with an error status, which ends the probe earlier.
const MAX_STREAMS: u32 = 16;

bitflags::bitflags! {
    /// Device feature bits the cell accepts: none. The transport itself adds
    /// `VERSION_1` and, when offered, `ACCESS_PLATFORM`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct SndFeatures: u64 {}
}

pub struct Device {
    // Declared before `_bars`: dropping the transport resets the device
    // while its BARs are still claimed.
    pub(crate) transport: VirtioPciTransport,
    _bars: Bars,
    pub(crate) dma: DmaArea,
    pub(crate) ctrl: SplitQueue,
    pub(crate) event: SplitQueue,
    pub(crate) tx: SplitQueue,
    rx: SplitQueue,
    /// The output stream played.
    pub stream: u32,
    /// START sent and not yet STOPped.
    pub(crate) running: bool,
    /// False once a request failed or the device misbehaved, until a reset.
    pub(crate) healthy: bool,
    /// Client PCM bytes in each tx slot the device holds (padding excluded).
    pub(crate) slot_bytes: [usize; PERIODS],
    /// Pacing and recovery state of [`crate::tx`].
    pub(crate) play: Playback,
}

/// The first virtio-sound function this cell may claim.
pub fn discover() -> Option<PcieDeviceInfo> {
    let mut info = PcieDeviceInfo::zeroed();
    for _ in 0..DISCOVERY_ATTEMPTS {
        if let Ok(true) = sys_find_pcie_device_by_vendor(VIRTIO_VENDOR_ID, DEVICE_ID, &mut info) {
            return Some(info);
        }
        ostd::task::yield_now();
    }
    None
}

/// `begin_init` with no device features; `Err` unless `FEATURES_OK` held.
fn negotiate(transport: &mut VirtioPciTransport) -> Result<(), InitError> {
    transport.begin_init(SndFeatures::empty());
    if transport.negotiated_ok() {
        Ok(())
    } else {
        Err(InitError::Negotiation(transport.init_failure()))
    }
}

/// A queue of `queue_size(max, wanted, chain)` entries in `chain` slots.
fn queue(
    t: &mut VirtioPciTransport,
    index: u16,
    wanted: u16,
    chain: u16,
) -> Result<SplitQueue, InitError> {
    let max = t.max_queue_size(index);
    let size = ring::queue_size(max, wanted, chain).ok_or(InitError::QueueSize(index, max))?;
    Ok(SplitQueue::new(index, size, chain))
}

/// Brings the function `record` describes to `DRIVER_OK` and picks the
/// output stream. Under DMA remapping the device must offer
/// `VIRTIO_F_ACCESS_PLATFORM`, since its DMA addresses are IOVAs.
pub fn bring_up(record: &ViPcieFunctionV1) -> Result<Device, InitError> {
    let policy = if record.flags & FLAG_DMA_REMAPPED != 0 {
        AccessPlatform::Required
    } else {
        AccessPlatform::Accept
    };
    let mut bars = Bars::new();
    let mut transport =
        VirtioPciTransport::new(record, &mut bars, policy).map_err(InitError::Transport)?;
    negotiate(&mut transport)?;
    let ctrl = queue(&mut transport, CONTROLQ, 4, 2)?;
    let event = queue(&mut transport, EVENTQ, EVENT_BUFS, 1)?;
    let tx = queue(&mut transport, TXQ, 2 * PERIODS as u16, 2)?;
    let rx = queue(&mut transport, RXQ, 2, 1)?;
    let dma = DmaArea::new(record.bdf).map_err(InitError::Dma)?;
    let mut device = Device {
        transport,
        _bars: bars,
        dma,
        ctrl,
        event,
        tx,
        rx,
        stream: 0,
        running: false,
        healthy: true,
        slot_bytes: [0; PERIODS],
        play: Playback::default(),
    };
    device.go_live()?;
    device.stream = device.find_stream()?;
    // Validates the format against the device before the first client.
    device
        .control(&proto::set_params(device.stream), &mut [0; STATUS_LEN])
        .map_err(InitError::Control)?;
    Ok(device)
}

impl Device {
    /// Programs every queue on a negotiated device, sets `DRIVER_OK` and
    /// posts the event buffers.
    fn go_live(&mut self) -> Result<(), InitError> {
        for q in [&mut self.ctrl, &mut self.event, &mut self.tx, &mut self.rx] {
            q.reset(&self.dma);
            let (desc, avail, used) = q.areas(&self.dma);
            let size = u32::from(q.ring.size);
            self.transport
                .queue_set(q.index, size, desc as usize, avail as usize, used as usize);
        }
        self.transport.finish_init();
        if let Some(failure) = self.transport.init_failure() {
            return Err(InitError::QueueSetup(failure));
        }
        for slot in 0..self.event.slots() {
            self.event
                .submit(&self.dma, slot, &[(ring::event_buf(slot), EVENT_LEN, true)]);
        }
        self.transport.notify(EVENTQ);
        self.running = false;
        self.healthy = true;
        self.slot_bytes = [0; PERIODS];
        Ok(())
    }

    /// Resets the device and brings it back to `DRIVER_OK` with the same
    /// queues and stream; in-flight tx buffers are lost.
    pub fn reinit(&mut self) -> Result<(), InitError> {
        self.healthy = false;
        negotiate(&mut self.transport)?;
        self.go_live()?;
        self.control(&proto::set_params(self.stream), &mut [0; STATUS_LEN])
            .map_err(InitError::Control)
    }

    /// The first output stream that plays the fixed format.
    fn find_stream(&mut self) -> Result<u32, InitError> {
        let mut resp = [0u8; STATUS_LEN + PCM_INFO_LEN];
        for stream in 0..MAX_STREAMS {
            match self.control(&proto::query_pcm_info(stream), &mut resp) {
                Ok(()) => {}
                Err(CtrlError::Status(_)) => break,
                Err(error) => return Err(InitError::Control(error)),
            }
            if PcmInfo::decode(&resp[STATUS_LEN..]).is_some_and(|info| info.plays_fixed_format()) {
                return Ok(stream);
            }
        }
        Err(InitError::NoStream)
    }
}
