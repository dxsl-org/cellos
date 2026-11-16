//! Intel 82574L bring-up and the polled-RX / interrupt-assisted-TX data path.
//!
//! Order (82574 datasheet §4.6 initialization): quiesce bus mastering, global
//! reset, mask interrupts, station address (`RAL0`/`RAH0` auto-loaded from NVM,
//! `EERD` words 0..3 as the fallback), interrupt routing, PHY autoneg restart,
//! DMA authorization, rings, TX enable, bounded link wait, RX enable. No
//! device-visible address is programmed before every DMA object is authorized.

use crate::identity::{self, Identity, MacKind, QUERY_DEVICE_IDS, VENDOR_INTEL};
use crate::irq;
use crate::regs::*;
use crate::reset::reset_and_read_mac;
use crate::rings::{RxDesc, TxDesc, BUF_SIZE, RX_SLOTS, TX_SLOTS};
use crate::{phy, rings};
use core::sync::atomic::{compiler_fence, Ordering};
use nic_driver::dma::{DmaSet, Layout};
use nic_driver::irq::BoundedWait;
use nic_driver::{MacFmt, NicDevice, Probe};
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

/// BAR0 register aperture of the 82574 (128 KiB).
const BAR0_LEN: usize = 0x2_0000;
/// Link wait: up to 50 parks (~5 s) with MSI-X, or this many spins when polled
/// (the igb lane's budget, which covers QEMU's ~500 ms autoneg timer).
const LINK_PARKS: u32 = 50;
const LINK_POLLS: u32 = 6_000_000;
/// TX completion: up to 20 parks (~2 s) with MSI-X, or this many spins.
const TX_PARKS: u32 = 20;
const TX_POLLS: u32 = 1_000_000;

pub struct E1000eController {
    mmio: MmioRegion,
    irq: BoundedWait,
    dma: DmaSet<TxDesc, RxDesc, TX_SLOTS, RX_SLOTS, 0>,
    tx_next: usize,
    rx_head: usize,
    /// Dropping the descriptors of a frame spread over several of them.
    rx_in_fragment: bool,
    mac: [u8; 6],
    /// Whether `STATUS.LU` was up before RX was enabled.
    link_up: bool,
    #[allow(dead_code)]
    kind: MacKind,
}

impl NicDevice for E1000eController {
    const TAG: &'static str = "[e1000e]";
    const VENDOR: u16 = VENDOR_INTEL;
    const QUERY_IDS: &'static [u16] = &QUERY_DEVICE_IDS;
    const ABSENT_LINE: Option<&'static str> = Some("no 82574 controller present; driver cell idle");
    type Identity = Identity;
    type Error = ViError;

    fn classify(vendor: u16, device: u16) -> Option<Identity> {
        identity::classify(vendor, device)
    }

    fn identity_name(id: &Identity) -> &'static str {
        id.name
    }

    fn init(probe: &Probe, id: Identity) -> ViResult<Self> {
        let mmio = probe.claim_bar0(BAR0_LEN, MMIO_MIN);
        Self::new(mmio, probe.bdf(), id.kind)
    }

    /// MSI-X entries held (0 when polled), why the cell is polled, and how many
    /// parks so far ended by an interrupt / by their deadline.
    fn bind_detail(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let vectors = self.irq.vectors().unwrap_or(0);
        let (irq, why) = if vectors > 0 {
            ("msix", "")
        } else {
            ("polled reason=", self.irq.reason())
        };
        write!(
            f,
            "link_up={} mac={} irq={}{} rx=request-driven vectors={} irq_wakes={} park_expiries={}",
            self.link_up,
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

impl E1000eController {
    /// Bring the controller up on `mmio` (BAR0) for the function at `bdf`.
    fn new(mmio: MmioRegion, bdf: u32, kind: MacKind) -> ViResult<Self> {
        let mac = reset_and_read_mac(&mmio, kind)?;

        let irq = BoundedWait::setup(Self::TAG, bdf, irq::WANT_VECTORS);
        irq::program(&mmio, irq.vectors())?;
        mmio.wr32(
            CTRL,
            (mmio.rd32(CTRL)? | CTRL_SLU | CTRL_ASDE) & !CTRL_GIO_MASTER_DISABLE,
        )?;
        phy::restart_autoneg(&mmio, kind)?;

        let dma = DmaSet::alloc_authorized(&Layout::PLAIN, bdf)?;
        let mut ctrl = E1000eController {
            mmio,
            irq,
            dma,
            tx_next: 0,
            rx_head: 0,
            rx_in_fragment: false,
            mac,
            link_up: false,
            kind,
        };
        ctrl.program_rings()?;
        Ok(ctrl)
    }

    /// Initial DMA-address programming and TX/RX enablement, in order: TX ring
    /// base, every RX descriptor, RX ring base, enable. Every address is an
    /// authorized IOVA.
    fn program_rings(&mut self) -> ViResult<()> {
        let iovas = self.dma.iovas;
        let m = &self.mmio;
        m.wr32(TDBAL, iovas.tx_ring as u32)?;
        m.wr32(TDBAH, (iovas.tx_ring >> 32) as u32)?;
        m.wr32(TDLEN, rings::ring_bytes(TX_SLOTS))?;
        m.wr32(TDH, 0)?;
        m.wr32(TDT, 0)?;

        for (slot, &iova) in iovas.rx_bufs.iter().enumerate() {
            rings::post_rx(&self.dma.rx_ring, slot, iova);
        }

        m.wr32(RDBAL, iovas.rx_ring as u32)?;
        m.wr32(RDBAH, (iovas.rx_ring >> 32) as u32)?;
        m.wr32(RDLEN, rings::ring_bytes(RX_SLOTS))?;
        m.wr32(RDH, 0)?;
        m.wr32(RDT, (RX_SLOTS - 1) as u32)?;

        m.wr32(TIPG, TIPG_COPPER)?;
        m.wr32(TCTL, TCTL_EN | TCTL_PSP | TCTL_CT | TCTL_COLD)?;
        // The receiver accepts frames only with link up, so wait for autoneg
        // (bounded) before enabling it; a timeout is reported in the bind
        // line, not fatal.
        self.link_up = self.irq.wait_for_rearmed(
            LINK_PARKS,
            LINK_POLLS,
            || irq::rearm(m),
            || Ok(m.rd32(STATUS)? & STATUS_LU != 0),
        )?;
        m.wr32(RCTL, RCTL_EN | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_SECRC)
    }

    /// Transmit `frame` (no FCS) and wait, bounded, for its write-back.
    /// `IO` when the slot to fill or the one after it was posted and never
    /// written back: the controller may still read the first, and posting
    /// would move the tail onto the second (one slot always stays empty).
    fn send_frame(&mut self, frame: &[u8]) -> ViResult<()> {
        if frame.is_empty() || frame.len() > BUF_SIZE {
            return Err(ViError::InvalidInput);
        }
        let slot = self.tx_next;
        if !rings::tx_has_room(&self.dma.tx_ring, slot, TX_SLOTS) {
            return Err(ViError::IO);
        }
        self.dma.tx_bufs.copy_in(slot, frame)?;
        self.tx_next = (slot + 1) % TX_SLOTS;
        let iova = self.dma.iovas.tx_bufs[slot];
        rings::post_tx(&self.dma.tx_ring, slot, iova, frame.len() as u16);
        compiler_fence(Ordering::Release);
        self.mmio.wr32(TDT, self.tx_next as u32)?;

        let (m, ring) = (&self.mmio, &self.dma.tx_ring);
        if self.irq.wait_for_rearmed(
            TX_PARKS,
            TX_POLLS,
            || irq::rearm(m),
            || Ok(rings::tx_done(ring, slot)),
        )? {
            Ok(())
        } else {
            Err(ViError::IO)
        }
    }

    /// Take the next received frame into `out`; 0 when none is ready.
    /// Errored and multi-descriptor frames are dropped.
    fn recv_frame(&mut self, out: &mut [u8]) -> usize {
        for _ in 0..RX_SLOTS {
            let head = self.rx_head;
            let length =
                match rings::rx_completed(&self.dma.rx_ring, head, &mut self.rx_in_fragment) {
                    rings::RxCompletion::Pending => return 0,
                    rings::RxCompletion::Frame(length) => Some(length.min(BUF_SIZE)),
                    rings::RxCompletion::Drop => None,
                };
            let copied = length.map(|len| self.dma.rx_bufs.copy_out(head, out, len));
            rings::post_rx(&self.dma.rx_ring, head, self.dma.iovas.rx_bufs[head]);
            self.rx_head = (head + 1) % RX_SLOTS;
            compiler_fence(Ordering::Release);
            let _ = self.mmio.wr32(RDT, head as u32);
            if let Some(copied) = copied {
                return copied;
            }
        }
        0
    }
}
