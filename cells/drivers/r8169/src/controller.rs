//! Realtek RTL8168/8125 controller bring-up and Tx/Rx data path.

use crate::identity::{self, decode_xid, Identity, RtlXid, QUERY_DEVICE_IDS, VENDOR_REALTEK};
use crate::phy;
use crate::regs::*;
use crate::rings::{self, RtlDesc, BUF_SIZE, RX_SLOTS, TX_SLOTS};
use core::sync::atomic::{compiler_fence, Ordering};
use nic_driver::dma::{DmaSet, Layout};
use nic_driver::{MacFmt, NicDevice, Probe};
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

const RTL_BAR0_LEN: usize = 0x1000;
const RTL_MMIO_MIN: usize = 0x0100;
const RESET_POLLS: u32 = 1_000_000;
const TX_POLLS: u32 = 1_000_000;
const LINK_POLLS: u32 = 6_000_000;

pub struct RtlController {
    mmio: MmioRegion,
    dma: DmaSet<RtlDesc, RtlDesc, TX_SLOTS, RX_SLOTS, 0>,
    tx_next: usize,
    rx_head: usize,
    mac: [u8; 6],
    link_up: bool,
    #[allow(dead_code)]
    xid: RtlXid,
}

impl NicDevice for RtlController {
    const TAG: &'static str = "[r8169]";
    const VENDOR: u16 = VENDOR_REALTEK;
    const QUERY_IDS: &'static [u16] = &QUERY_DEVICE_IDS;
    const ABSENT_LINE: Option<&'static str> =
        Some("no RTL8168/8125 controller present; driver cell idle");
    type Identity = Identity;
    type Error = ViError;

    fn classify(vendor: u16, device: u16) -> Option<Identity> {
        identity::classify(vendor, device)
    }

    fn identity_name(id: &Identity) -> &'static str {
        id.name
    }

    fn init(probe: &Probe, _id: Identity) -> ViResult<Self> {
        let mmio = probe.claim_bar0(RTL_BAR0_LEN, RTL_MMIO_MIN);
        Self::new(mmio, probe.bdf())
    }

    fn bind_detail(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "link_up={} mac={}", self.link_up, MacFmt(self.mac))
    }

    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn send(&mut self, frame: &[u8]) -> ViResult<()> {
        self.send_frame(frame)
    }

    fn recv(&mut self, out: &mut [u8]) -> usize {
        self.poll_rx(out).unwrap_or_default()
    }
}

impl RtlController {
    fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        // 1. Quiesce & reset controller.
        mmio.write_u8(COMMAND, CR_RST)?;
        let mut polls = 0u32;
        while mmio.read_u8(COMMAND)? & CR_RST != 0 {
            polls += 1;
            if polls > RESET_POLLS {
                return Err(ViError::IO);
            }
            core::hint::spin_loop();
        }

        // 2. Decode XID and read station MAC.
        let txcfg = mmio.read_u32(TXCFG)?;
        let xid = decode_xid(txcfg);
        let mac = [
            mmio.read_u8(IDR0)?,
            mmio.read_u8(IDR0 + 1)?,
            mmio.read_u8(IDR0 + 2)?,
            mmio.read_u8(IDR0 + 3)?,
            mmio.read_u8(IDR0 + 4)?,
            mmio.read_u8(IDR0 + 5)?,
        ];
        if mac == [0u8; 6] {
            return Err(ViError::IO);
        }

        // 3. Mask interrupts and restart PHY autoneg.
        mmio.write::<u16>(IMR, 0)?;
        mmio.write::<u16>(ISR, u16::MAX)?;
        phy::restart_autoneg(&mmio)?;

        // 4. Allocate and authorize DMA; nothing is programmed before this.
        let dma = DmaSet::alloc_authorized(&Layout::PLAIN, bdf)?;
        let mut ctrl = RtlController {
            mmio,
            dma,
            tx_next: 0,
            rx_head: 0,
            mac,
            link_up: false,
            xid,
        };
        ctrl.program_rings()?;
        ctrl.finish_init()?;
        Ok(ctrl)
    }

    /// Initial DMA programming: TX list, RX list, then every RX descriptor,
    /// all from authorized IOVAs.
    fn program_rings(&mut self) -> ViResult<()> {
        let iovas = self.dma.iovas;
        self.mmio.write_u32(TXLIST_ADDR_LO, iovas.tx_ring as u32)?;
        self.mmio
            .write_u32(TXLIST_ADDR_HI, (iovas.tx_ring >> 32) as u32)?;
        self.mmio.write_u32(RXLIST_ADDR_LO, iovas.rx_ring as u32)?;
        self.mmio
            .write_u32(RXLIST_ADDR_HI, (iovas.rx_ring >> 32) as u32)?;
        for (index, &iova) in iovas.rx_bufs.iter().enumerate() {
            self.dma
                .rx_ring
                .write(index, RtlDesc::rx_posted(index, iova));
        }
        Ok(())
    }

    fn finish_init(&mut self) -> ViResult<()> {
        self.mmio.write::<u16>(
            CPLUS_CMD,
            CPLUSCMD_PCI_MULRW | CPLUSCMD_RXENB | CPLUSCMD_TXENB,
        )?;
        self.mmio.write_u32(TXCFG, TXCFG_IFG | TXCFG_MAXDMA)?;
        self.mmio
            .write_u32(RXCFG, RXCFG_MAXDMA | RXCFG_ACCEPT_ALL)?;
        self.mmio.write_u8(COMMAND, CR_RE | CR_TE)?;

        for _ in 0..LINK_POLLS {
            if phy::phy_read(&self.mmio, phy::MII_BMCR)? & 0x04 != 0 {
                self.link_up = true;
                break;
            }
            core::hint::spin_loop();
        }
        Ok(())
    }

    fn send_frame(&mut self, frame: &[u8]) -> ViResult<()> {
        if frame.is_empty() || frame.len() > BUF_SIZE {
            return Err(ViError::InvalidArgument);
        }
        let slot = self.tx_next;
        self.dma.tx_bufs.copy_in(slot, frame)?;
        compiler_fence(Ordering::Release);

        let iova = self.dma.iovas.tx_bufs[slot];
        let mut opts = rings::DESC_OWN | rings::DESC_FS | rings::DESC_LS | (frame.len() as u32);
        if slot == TX_SLOTS - 1 {
            opts |= rings::DESC_RING_END;
        }
        self.dma.tx_ring.write(
            slot,
            RtlDesc {
                opts1: opts,
                opts2: 0,
                addr_lo: iova as u32,
                addr_hi: (iova >> 32) as u32,
            },
        );
        compiler_fence(Ordering::Release);

        self.tx_next = (slot + 1) % TX_SLOTS;
        self.mmio.write_u8(TPPOLL, TPPOLL_NPQ)?;

        for _ in 0..TX_POLLS {
            compiler_fence(Ordering::Acquire);
            if self.dma.tx_ring.read(slot).opts1 & rings::DESC_OWN == 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(ViError::IO)
    }

    fn poll_rx(&mut self, target: &mut [u8]) -> ViResult<usize> {
        let slot = self.rx_head;
        compiler_fence(Ordering::Acquire);
        let desc = self.dma.rx_ring.read(slot);
        if desc.opts1 & rings::DESC_OWN != 0 {
            return Ok(0);
        }
        let len = self
            .dma
            .rx_bufs
            .copy_out(slot, target, (desc.opts1 & 0x3FFF) as usize);

        self.dma
            .rx_ring
            .write(slot, RtlDesc::rx_posted(slot, self.dma.iovas.rx_bufs[slot]));
        compiler_fence(Ordering::Release);
        self.rx_head = (slot + 1) % RX_SLOTS;
        Ok(len)
    }
}
