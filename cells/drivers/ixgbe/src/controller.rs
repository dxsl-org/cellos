//! Intel 82599/X540/X550 controller initialization.

use crate::identity::{self, Identity, IxgbeMac, QUERY_DEVICE_IDS, VENDOR_INTEL};
use crate::regs::*;
use crate::rings::{RxDesc, TxDesc, RX_SLOTS, TX_SLOTS};
use crate::sfp;
use nic_driver::dma::{DmaSet, Layout};
use nic_driver::{MacFmt, NicDevice, Probe};
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

const IXGBE_BAR0_LEN: usize = 0x2_0000;
const IXGBE_MMIO_MIN: usize = 0x1_0018;
const RESET_POLLS: u32 = 1_000_000;
const LINK_POLLS: u32 = 6_000_000;

pub struct IxgbeController {
    pub(crate) mmio: MmioRegion,
    pub(crate) dma: DmaSet<TxDesc, RxDesc, TX_SLOTS, RX_SLOTS, 0>,
    pub(crate) tx_next: usize,
    pub(crate) rx_head: usize,
    mac: [u8; 6],
    link_up: bool,
    #[allow(dead_code)]
    mac_type: IxgbeMac,
}

impl NicDevice for IxgbeController {
    const TAG: &'static str = "[ixgbe]";
    const VENDOR: u16 = VENDOR_INTEL;
    const QUERY_IDS: &'static [u16] = &QUERY_DEVICE_IDS;
    const ABSENT_LINE: Option<&'static str> =
        Some("no 82599/X540/X550 controller present; driver cell idle");
    type Identity = Identity;
    type Error = ViError;

    fn classify(vendor: u16, device: u16) -> Option<Identity> {
        identity::classify(vendor, device)
    }

    fn identity_name(id: &Identity) -> &'static str {
        id.name
    }

    fn init(probe: &Probe, id: Identity) -> ViResult<Self> {
        let mmio = probe.claim_bar0(IXGBE_BAR0_LEN, IXGBE_MMIO_MIN);
        Self::new(mmio, probe.bdf(), id.mac)
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

impl IxgbeController {
    fn new(mmio: MmioRegion, bdf: u32, mac_type: IxgbeMac) -> ViResult<Self> {
        // 1. Quiesce & reset controller.
        mmio.write_u32(CTRL, CTRL_RST)?;
        let mut polls = 0u32;
        while mmio.read_u32(CTRL)? & CTRL_RST != 0 {
            polls += 1;
            if polls > RESET_POLLS {
                return Err(ViError::IO);
            }
            core::hint::spin_loop();
        }

        // 2. SFP module check for 82599.
        if mac_type == IxgbeMac::Mac82599 {
            let _ = sfp::identify_sfp(&mmio);
        }

        let mac = match (mmio.read_u32(RAL0)?, mmio.read_u32(RAH0)?) {
            (ral, rah) if rah & RAH_AV != 0 && ral != 0 => [
                ral as u8,
                (ral >> 8) as u8,
                (ral >> 16) as u8,
                (ral >> 24) as u8,
                rah as u8,
                (rah >> 8) as u8,
            ],
            _ => {
                let (w0, w1, w2) = (
                    nvm_read(&mmio, 0)?,
                    nvm_read(&mmio, 1)?,
                    nvm_read(&mmio, 2)?,
                );
                [
                    w0 as u8,
                    (w0 >> 8) as u8,
                    w1 as u8,
                    (w1 >> 8) as u8,
                    w2 as u8,
                    (w2 >> 8) as u8,
                ]
            }
        };
        if mac == [0u8; 6] {
            return Err(ViError::IO);
        }

        mmio.write_u32(RAL0, u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]))?;
        mmio.write_u32(RAH0, RAH_AV | (u16::from_le_bytes([mac[4], mac[5]]) as u32))?;

        // Every ring and buffer is authorized before any address below is
        // programmed into the controller.
        let dma = DmaSet::alloc_authorized(&Layout::PLAIN, bdf)?;
        let mut ctrl = IxgbeController {
            mmio,
            dma,
            tx_next: 0,
            rx_head: 0,
            mac,
            link_up: false,
            mac_type,
        };
        ctrl.program_rings()?;
        ctrl.finish_init()?;
        Ok(ctrl)
    }

    /// Initial DMA programming: TX ring base, RX ring base, then every RX
    /// descriptor, all from authorized IOVAs.
    fn program_rings(&mut self) -> ViResult<()> {
        let iovas = self.dma.iovas;
        let m = &self.mmio;
        m.write_u32(TDBAL0, iovas.tx_ring as u32)?;
        m.write_u32(TDBAH0, (iovas.tx_ring >> 32) as u32)?;
        m.write_u32(TDLEN0, (TX_SLOTS * 16) as u32)?;
        m.write_u32(TDH0, 0)?;
        m.write_u32(TDT0, 0)?;
        m.write_u32(RDBAL0, iovas.rx_ring as u32)?;
        m.write_u32(RDBAH0, (iovas.rx_ring >> 32) as u32)?;
        m.write_u32(RDLEN0, (RX_SLOTS * 16) as u32)?;
        m.write_u32(RDH0, 0)?;
        m.write_u32(RDT0, 0)?;
        for (index, &iova) in iovas.rx_bufs.iter().enumerate() {
            self.dma.rx_ring.write(index, RxDesc::posted(iova));
        }
        Ok(())
    }

    fn finish_init(&mut self) -> ViResult<()> {
        self.mmio.write_u32(RXDCTL0, RXDCTL_ENABLE)?;
        self.mmio.write_u32(TXDCTL0, TXDCTL_ENABLE)?;
        self.mmio.write_u32(DMATXCTL, DMATXCTL_TE)?;
        self.mmio.write_u32(RDT0, (RX_SLOTS - 1) as u32)?;
        self.mmio.write_u32(RXCTRL, RXCTRL_RXEN)?;
        self.mmio
            .write_u32(FCTRL, FCTRL_BAM | FCTRL_MPE | FCTRL_UPE)?;

        let mut polls = 0u32;
        while self.mmio.read_u32(STATUS)? & STATUS_LU == 0 {
            polls += 1;
            if polls > LINK_POLLS {
                break;
            }
            core::hint::spin_loop();
        }
        self.link_up = self.mmio.read_u32(STATUS)? & STATUS_LU != 0;
        Ok(())
    }
}
