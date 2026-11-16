//! Intel I225/I226 controller initialization and Tx/Rx data path.

use crate::identity::{self, Identity, QUERY_DEVICE_IDS, VENDOR_INTEL};
use crate::phy;
use crate::regs::*;
use crate::rings::{self, RxDesc, TxDesc, BUF_SIZE, RX_SLOTS, TX_SLOTS};
use core::sync::atomic::{compiler_fence, Ordering};
use nic_driver::dma::{DmaSet, Layout};
use nic_driver::{MacFmt, NicDevice, Probe};
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

const IGC_BAR0_LEN: usize = 0x2_0000;
const IGC_MMIO_MIN: usize = 0x0_E040;
const RESET_POLLS: u32 = 1_000_000;
const TX_POLLS: u32 = 1_000_000;
const LINK_POLLS: u32 = 6_000_000;

pub struct IgcController {
    mmio: MmioRegion,
    dma: DmaSet<TxDesc, RxDesc, TX_SLOTS, RX_SLOTS, 0>,
    tx_next: usize,
    rx_head: usize,
    mac: [u8; 6],
    link_up: bool,
}

impl NicDevice for IgcController {
    const TAG: &'static str = "[igc]";
    const VENDOR: u16 = VENDOR_INTEL;
    const QUERY_IDS: &'static [u16] = &QUERY_DEVICE_IDS;
    const ABSENT_LINE: Option<&'static str> =
        Some("no I225/I226 controller present; driver cell idle");
    type Identity = Identity;
    type Error = ViError;

    fn classify(vendor: u16, device: u16) -> Option<Identity> {
        identity::classify(vendor, device)
    }

    fn identity_name(id: &Identity) -> &'static str {
        id.name
    }

    fn init(probe: &Probe, _id: Identity) -> ViResult<Self> {
        let mmio = probe.claim_bar0(IGC_BAR0_LEN, IGC_MMIO_MIN);
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

impl IgcController {
    fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        mmio.write_u32(CTRL, CTRL_RST)?;
        let mut polls = 0u32;
        while mmio.read_u32(CTRL)? & CTRL_RST != 0 {
            polls += 1;
            if polls > RESET_POLLS {
                return Err(ViError::IO);
            }
            core::hint::spin_loop();
        }

        mmio.write_u32(EEER, 0)?;
        mmio.write_u32(EEE_SU, 0)?;
        phy::restart_autoneg(&mmio)?;

        mmio.write_u32(CTRL, CTRL_SLU | CTRL_ASDE)?;
        mmio.write_u32(IMC, u32::MAX)?;
        let _ = mmio.read_u32(ICR)?;

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

        let ral_val = u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]);
        let rah_val = RAH_AV | (u16::from_le_bytes([mac[4], mac[5]]) as u32);
        mmio.write_u32(RAL0, ral_val)?;
        mmio.write_u32(RAH0, rah_val)?;
        for index in 0..MTA_ENTRIES {
            mmio.write_u32(MTA + index * 4, 0)?;
        }

        // Every ring and buffer is authorized before any address below is
        // programmed into the controller.
        let dma = DmaSet::alloc_authorized(&Layout::PLAIN, bdf)?;
        let mut ctrl = IgcController {
            mmio,
            dma,
            tx_next: 0,
            rx_head: 0,
            mac,
            link_up: false,
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
        self.mmio
            .write_u32(SRRCTL0, SRRCTL_DESCTYPE_ADV_ONEBUF | 2)?;
        self.mmio.write_u32(RXDCTL0, RXDCTL_ENABLE)?;
        self.mmio.write_u32(TXDCTL0, TXDCTL_ENABLE)?;
        self.mmio.write_u32(TIPG, TIPG_COPPER)?;
        self.mmio
            .write_u32(TCTL, TCTL_EN | TCTL_PSP | TCTL_CT | TCTL_COLD)?;
        self.mmio.write_u32(RDT0, (RX_SLOTS - 1) as u32)?;

        let mut polls = 0u32;
        while self.mmio.read_u32(STATUS)? & STATUS_LU == 0 {
            polls += 1;
            if polls > LINK_POLLS {
                break;
            }
            core::hint::spin_loop();
        }
        self.link_up = self.mmio.read_u32(STATUS)? & STATUS_LU != 0;
        self.mmio
            .write_u32(RCTL, RCTL_EN | RCTL_BAM | RCTL_UPE | RCTL_MPE | RCTL_SECRC)
    }

    fn send_frame(&mut self, frame: &[u8]) -> ViResult<()> {
        if frame.is_empty() || frame.len() > BUF_SIZE {
            return Err(ViError::InvalidArgument);
        }
        let slot = self.tx_next;
        self.dma.tx_bufs.copy_in(slot, frame)?;
        compiler_fence(Ordering::Release);

        let iova = self.dma.iovas.tx_bufs[slot];
        self.dma.tx_ring.write(
            slot,
            TxDesc {
                buffer_addr: iova,
                cmd_type_len: rings::ADVTXD_DCMD_DEXT
                    | rings::ADVTXD_DTYP_DATA
                    | rings::TXD_CMD_EOP
                    | rings::TXD_CMD_IFCS
                    | rings::TXD_CMD_RS
                    | (frame.len() as u32),
                olinfo_status: (frame.len() as u32) << 16,
            },
        );
        compiler_fence(Ordering::Release);
        self.tx_next = (slot + 1) % TX_SLOTS;
        self.mmio.write_u32(TDT0, self.tx_next as u32)?;

        for _ in 0..TX_POLLS {
            compiler_fence(Ordering::Acquire);
            if self.dma.tx_ring.read(slot).cmd_type_len & rings::TXD_STAT_DD != 0 {
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
        if desc.status_error & rings::RXD_STAT_DD == 0 {
            return Ok(0);
        }
        let len = self
            .dma
            .rx_bufs
            .copy_out(slot, target, desc.length as usize);

        self.dma
            .rx_ring
            .write(slot, RxDesc::posted(self.dma.iovas.rx_bufs[slot]));
        compiler_fence(Ordering::Release);
        self.rx_head = (slot + 1) % RX_SLOTS;
        self.mmio.write_u32(RDT0, slot as u32)?;
        Ok(len)
    }
}
