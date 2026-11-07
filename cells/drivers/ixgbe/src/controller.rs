//! Intel 82599/X540/X550 controller initialization and Tx/Rx data path.

use crate::dma_layout::{
    for_each_initial_dma_program, try_init_array, with_authorized_dma_layout, DmaIovas, DmaSlot,
    InitialDmaProgram, RX_SLOTS, TX_SLOTS,
};
use crate::identity::IxgbeMac;
use crate::regs::*;
use crate::rings::{self, RxDesc};
use crate::sfp;
use ostd::dma::DmaBuf;
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

pub use crate::rings::BUF_SIZE;

const RESET_POLLS: u32 = 1_000_000;
const LINK_POLLS: u32 = 6_000_000;

pub struct IxgbeController {
    pub(crate) mmio: MmioRegion,
    pub(crate) tx_ring: DmaBuf,
    pub(crate) rx_ring: DmaBuf,
    pub(crate) tx_bufs: [DmaBuf; TX_SLOTS],
    pub(crate) rx_bufs: [DmaBuf; RX_SLOTS],
    pub(crate) dma_iovas: DmaIovas,
    pub(crate) tx_next: usize,
    pub(crate) rx_head: usize,
    pub mac: [u8; 6],
    pub link_up: bool,
    #[allow(dead_code)]
    pub mac_type: IxgbeMac,
}

unsafe impl Send for IxgbeController {}

impl IxgbeController {
    pub fn new(mmio: MmioRegion, bdf: u32, mac_type: IxgbeMac) -> ViResult<Self> {
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

        let tx_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
        let tx_bufs = try_init_array(|_| DmaBuf::alloc(1).ok_or(ViError::OutOfMemory))?;
        let rx_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
        let rx_bufs = try_init_array(|_| DmaBuf::alloc(1).ok_or(ViError::OutOfMemory))?;

        with_authorized_dma_layout(
            (mmio, tx_ring, rx_ring, tx_bufs, rx_bufs),
            |r, s| {
                match s {
                    DmaSlot::TxRing => r.1.authorize(bdf),
                    DmaSlot::TxBuffer(i) => r.3[i].authorize(bdf),
                    DmaSlot::RxRing => r.2.authorize(bdf),
                    DmaSlot::RxBuffer(i) => r.4[i].authorize(bdf),
                }
                .map_err(|_| ViError::PermissionDenied)
            },
            |(mmio, tx_ring, rx_ring, tx_bufs, rx_bufs), dma_iovas| {
                rings::clear(&tx_ring);
                rings::clear(&rx_ring);
                let mut ctrl = IxgbeController {
                    mmio,
                    tx_ring,
                    rx_ring,
                    tx_bufs,
                    rx_bufs,
                    dma_iovas,
                    tx_next: 0,
                    rx_head: 0,
                    mac,
                    link_up: false,
                    mac_type,
                };
                for_each_initial_dma_program(&dma_iovas, |p| {
                    let _ = ctrl.apply(p);
                });
                let _ = ctrl.finish_init();
                Ok(ctrl)
            },
        )?
    }

    fn apply(&mut self, program: InitialDmaProgram) -> ViResult<()> {
        match program {
            InitialDmaProgram::TxRingBase(iova) => {
                self.mmio.write_u32(TDBAL0, iova as u32)?;
                self.mmio.write_u32(TDBAH0, (iova >> 32) as u32)?;
                self.mmio.write_u32(TDLEN0, (TX_SLOTS * 16) as u32)?;
                self.mmio.write_u32(TDH0, 0)?;
                self.mmio.write_u32(TDT0, 0)?;
            }
            InitialDmaProgram::RxRingBase(iova) => {
                self.mmio.write_u32(RDBAL0, iova as u32)?;
                self.mmio.write_u32(RDBAH0, (iova >> 32) as u32)?;
                self.mmio.write_u32(RDLEN0, (RX_SLOTS * 16) as u32)?;
                self.mmio.write_u32(RDH0, 0)?;
                self.mmio.write_u32(RDT0, 0)?;
            }
            InitialDmaProgram::RxBuffer { index, iova } => rings::write_rx_desc(
                &self.rx_ring,
                index,
                RxDesc {
                    buffer_addr: iova,
                    status_error: 0,
                    length: 0,
                    vlan: 0,
                },
            ),
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
