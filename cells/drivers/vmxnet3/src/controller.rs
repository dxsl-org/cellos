//! vmxnet3 bring-up (the data path is in `datapath.rs`).
//!
//! Order: revision negotiation, device reset, permanent MAC, MSI-X (435), one
//! DMA allocation authorized for the BDF before any of its addresses is
//! written anywhere, RX ring 0 filled, queue descriptors and `DriverShared`
//! written, `DriverShared` address programmed, `ACTIVATE_DEV`, RX producers,
//! receive filters, vector masks, bounded link wait. The device is reset
//! before `GrantDma` turns bus mastering on; the kernel turns memory decoding
//! on when the cell claims the function (`FindPcieDevice*`), so the first
//! register read does not depend on firmware.

use crate::datapath::ack_events;
use crate::dma::DmaArea;
use crate::identity::{self, Identity, QUERY_DEVICE_IDS, VENDOR_VMWARE};
use crate::irq::{IntrPlan, WANT_VECTORS};
use crate::regs::*;
use crate::rings::*;
use crate::shared::{write_driver_shared, write_queues, QueueConfig};
use abi::pcie_function::{PcieBar, BAR_KIND_MEM32, BAR_KIND_MEM64};
use core::sync::atomic::{fence, Ordering};
use nic_driver::irq::BoundedWait;
use nic_driver::{MacFmt, NicDevice, Probe};
use ostd::mmio::{self, MmioRegion};
use ostd::syscall::sys_query_pcie_function;
use types::{ViError, ViResult};

/// Link wait: up to 50 parks (~5 s) with MSI-X, or this many polls.
const LINK_PARKS: u32 = 50;
const LINK_POLLS: u32 = 2_000_000;

/// Why bring-up stopped. `Debug` (what the skeleton prints) is the `Display`
/// text.
#[derive(Clone, Copy)]
pub enum InitError {
    Mmio(ViError),
    Refused(Refusal),
    /// The permanent MAC is zero or multicast.
    Mac([u8; 6]),
    /// DMA allocation or authorization failed.
    Dma,
    /// `ACTIVATE_DEV` returned this non-zero status.
    Activate(u32),
}

impl From<ViError> for InitError {
    fn from(error: ViError) -> Self {
        Self::Mmio(error)
    }
}

impl core::fmt::Display for InitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Mmio(error) => write!(f, "register access: {error:?}"),
            Self::Refused(Refusal::Revision(v)) => write!(f, "no revision 1 (VRRS={v:#x})"),
            Self::Refused(Refusal::Upt(v)) => write!(f, "no UPT version 1 (UVRS={v:#x})"),
            Self::Mac(mac) => write!(f, "unusable MAC {}", MacFmt(*mac)),
            Self::Dma => write!(f, "DMA allocation/authorization failed"),
            Self::Activate(status) => write!(f, "ACTIVATE_DEV status {status:#x}"),
        }
    }
}

impl core::fmt::Debug for InitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(self, f)
    }
}

pub struct Vmxnet3 {
    pub(crate) pt: MmioRegion,
    vd: MmioRegion,
    pub(crate) dma: DmaArea,
    pub(crate) irq: BoundedWait,
    pub(crate) tx: GenRing,
    pub(crate) tx_comp: GenRing,
    /// Next TX slot the device has not completed.
    pub(crate) tx_clean: usize,
    pub(crate) rx_fill: GenRing,
    pub(crate) rx_comp: GenRing,
    mac: [u8; 6],
    link_up: bool,
    link_mbps: u32,
}

impl NicDevice for Vmxnet3 {
    const TAG: &'static str = "[vmxnet3]";
    const VENDOR: u16 = VENDOR_VMWARE;
    const QUERY_IDS: &'static [u16] = &QUERY_DEVICE_IDS;
    const ABSENT_LINE: Option<&'static str> =
        Some("no vmxnet3 controller present; driver cell idle");
    type Identity = Identity;
    type Error = InitError;

    fn classify(vendor: u16, device: u16) -> Option<Identity> {
        identity::classify(vendor, device)
    }

    fn identity_name(id: &Identity) -> &'static str {
        id.name
    }

    /// Claim BAR0 (PT) and BAR1 (VD) from the 434 function record, then bring
    /// the device up.
    fn init(probe: &Probe, _id: Identity) -> Result<Self, InitError> {
        let bdf = probe.bdf();
        let record = match sys_query_pcie_function(bdf) {
            Ok(record) => record,
            Err(error) => probe.fail(format_args!("query {bdf:#x} failed: {error:?}")),
        };
        let pt = claim(probe, 0, &record.bars[0], PT_MIN);
        let vd = claim(probe, 1, &record.bars[1], VD_MIN);
        Self::new(pt, vd, bdf)
    }

    /// Revision, link, MAC, and the interrupt evidence: MSI-X entries held (0
    /// when polled), why the cell is polled, and how many parks so far ended
    /// by an interrupt / by their deadline.
    fn bind_detail(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let vectors = self.irq.vectors().unwrap_or(0);
        let (irq, why) = if vectors > 0 {
            ("msix", "")
        } else {
            ("polled reason=", self.irq.reason())
        };
        write!(
            f,
            "rev=1 link_up={} speed={}Mb/s mac={} irq={}{} rx=request-driven vectors={} irq_wakes={} park_expiries={}",
            self.link_up,
            self.link_mbps,
            MacFmt(self.mac),
            irq,
            why,
            vectors,
            self.irq.wakes(),
            self.irq.expiries()
        )
    }

    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn send(&mut self, frame: &[u8]) -> ViResult<()> {
        self.send_frame(frame)
    }

    fn recv(&mut self, out: &mut [u8]) -> usize {
        self.recv_frame(out)
    }
}

/// Claim memory BAR `index` when it holds at least `min` bytes; exits 1
/// otherwise.
fn claim(probe: &Probe, index: usize, bar: &PcieBar, min: usize) -> MmioRegion {
    let memory = bar.kind == BAR_KIND_MEM32 || bar.kind == BAR_KIND_MEM64;
    let len = usize::try_from(bar.len).unwrap_or(0);
    if !memory || bar.base == 0 || len < min {
        probe.fail(format_args!(
            "BAR{index} unusable (kind={} base={:#x} len={:#x})",
            bar.kind, bar.base, bar.len
        ));
    }
    match mmio::request_region(bar.base as usize, len) {
        Ok(region) => region,
        Err(error) => probe.fail(format_args!("BAR{index} claim failed: {error:?}")),
    }
}

impl Vmxnet3 {
    /// Bring the device up on BAR0 `pt` and BAR1 `vd` for the function `bdf`.
    fn new(pt: MmioRegion, vd: MmioRegion, bdf: u32) -> Result<Self, InitError> {
        negotiate(&vd)?.map_err(InitError::Refused)?;
        vd.wr32(VD_CMD, CMD_RESET_DEV)?;
        let mac = permanent_mac(&vd)?;
        if !mac_usable(mac) {
            return Err(InitError::Mac(mac));
        }
        let irq = BoundedWait::setup(Self::TAG, bdf, WANT_VECTORS);
        let plan = IntrPlan::for_vectors(irq.vectors());
        let dma = DmaArea::new(DMA_PAGES, bdf).ok_or(InitError::Dma)?;
        // From here on, dropping `ctrl` quiesces and resets the device.
        let mut ctrl = Self {
            pt,
            vd,
            dma,
            irq,
            tx: GenRing::new(TX_SLOTS),
            tx_comp: GenRing::new(TX_SLOTS),
            tx_clean: 0,
            rx_fill: GenRing::new(RX_SLOTS),
            rx_comp: GenRing::new(RX_COMP_SLOTS),
            mac,
            link_up: false,
            link_mbps: 0,
        };
        ctrl.configure(plan)?;
        ctrl.wait_link()?;
        Ok(ctrl)
    }

    fn configure(&mut self, plan: IntrPlan) -> Result<(), InitError> {
        let dma = &mut self.dma;
        dma.zero(0, DMA_PAGES * 4096);
        // One slot stays empty so "producer == consumer" always means empty.
        for _ in 0..RX_SLOTS - 1 {
            let slot = self.rx_fill.next;
            let addr = dma.iova(RX_BUF_OFF + slot * BUF_SIZE);
            post_rx(dma, slot, addr, self.rx_fill.gen);
            self.rx_fill.advance();
        }
        let queues = QueueConfig {
            tx_ring: dma.iova(TX_RING_OFF),
            tx_data: dma.iova(TX_DATA_OFF),
            tx_comp: dma.iova(TX_COMP_OFF),
            rx_ring: [dma.iova(RX_RING_OFF[0]), dma.iova(RX_RING_OFF[1])],
            rx_comp: dma.iova(RX_COMP_OFF),
            tx_len: TX_SLOTS as u32,
            rx_len: [RX_SLOTS as u32; 2],
            rx_comp_len: RX_COMP_SLOTS as u32,
        };
        write_queues(dma, QUEUES_OFF, &queues, plan);
        let queues_iova = dma.iova(QUEUES_OFF);
        write_driver_shared(dma, SHARED_OFF, queues_iova, plan);
        fence(Ordering::SeqCst);

        let (pt, vd) = (&self.pt, &self.vd);
        set_mac(vd, self.mac)?;
        let shared = self.dma.iova(SHARED_OFF);
        vd.wr32(VD_DSL, shared as u32)?;
        vd.wr32(VD_DSH, (shared >> 32) as u32)?;
        let status = query(vd, CMD_ACTIVATE_DEV)?;
        if status != 0 {
            return Err(InitError::Activate(status));
        }
        pt.wr32(PT_RXPROD, self.rx_fill.next as u32)?;
        pt.wr32(PT_RXPROD2, 0)?;
        for command in [
            CMD_UPDATE_RX_MODE,
            CMD_UPDATE_MAC_FILTERS,
            CMD_UPDATE_VLAN_FILTERS,
        ] {
            vd.wr32(VD_CMD, command)?;
        }
        // PT writes are ignored before activation, so the masks go last. RX
        // stays masked unless it shares the TX or the event vector.
        if self.irq.vectors().is_some() {
            pt.wr32(pt_imask(plan.tx), 0)?;
            pt.wr32(pt_imask(plan.event), 0)?;
            if plan.masks_rx() {
                pt.wr32(pt_imask(plan.rx), 1)?;
            }
        } else {
            pt.wr32(pt_imask(0), 1)?;
        }
        Ok(())
    }

    fn wait_link(&mut self) -> ViResult<()> {
        let (vd, dma) = (&self.vd, &self.dma);
        let mut link = 0;
        self.link_up = self.irq.wait_for(LINK_PARKS, LINK_POLLS, || {
            ack_events(vd, dma)?;
            link = query(vd, CMD_GET_LINK)?;
            Ok(link & LINK_UP != 0)
        })?;
        self.link_mbps = link >> 16;
        Ok(())
    }
}

impl Drop for Vmxnet3 {
    /// Stop all DMA before the cell lets go of the device.
    fn drop(&mut self) {
        let _ = self.vd.wr32(VD_CMD, CMD_QUIESCE_DEV);
        let _ = self.vd.wr32(VD_CMD, CMD_RESET_DEV);
    }
}
