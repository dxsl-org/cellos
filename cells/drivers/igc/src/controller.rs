//! Intel I225/I226 controller initialization and Tx/Rx data path.

use crate::dma_layout::{
    for_each_initial_dma_program, try_init_array, with_authorized_dma_layout, DmaIovas, DmaSlot,
    InitialDmaProgram, RX_SLOTS, TX_SLOTS,
};
use crate::phy;
use crate::regs::*;
use crate::rings::{self, RxDesc, TxDesc};
use core::sync::atomic::{compiler_fence, Ordering};
use ostd::dma::DmaBuf;
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

pub use crate::rings::BUF_SIZE;

const RESET_POLLS: u32 = 1_000_000;
const TX_POLLS: u32 = 1_000_000;
const LINK_POLLS: u32 = 6_000_000;

pub struct IgcController {
    mmio: MmioRegion,
    tx_ring: DmaBuf,
    rx_ring: DmaBuf,
    tx_bufs: [DmaBuf; TX_SLOTS],
    rx_bufs: [DmaBuf; RX_SLOTS],
    dma_iovas: DmaIovas,
    tx_next: usize,
    rx_head: usize,
    pub mac: [u8; 6],
    pub link_up: bool,
}

unsafe impl Send for IgcController {}

impl IgcController {
    pub fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
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
                let mut ctrl = IgcController {
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

    pub fn send_frame(&mut self, frame: &[u8]) -> ViResult<()> {
        if frame.is_empty() || frame.len() > BUF_SIZE {
            return Err(ViError::InvalidArgument);
        }
        let slot = self.tx_next;
        let buf = &self.tx_bufs[slot];
        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), buf.virt(), frame.len());
        }
        compiler_fence(Ordering::Release);

        let iova = self.dma_iovas.tx_bufs[slot];
        rings::write_tx_desc(
            &self.tx_ring,
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
            if rings::read_tx_desc(&self.tx_ring, slot).cmd_type_len & rings::TXD_STAT_DD != 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(ViError::IO)
    }

    pub fn poll_rx(&mut self, target: &mut [u8]) -> ViResult<usize> {
        let slot = self.rx_head;
        compiler_fence(Ordering::Acquire);
        let desc = rings::read_rx_desc(&self.rx_ring, slot);
        if desc.status_error & rings::RXD_STAT_DD == 0 {
            return Ok(0);
        }
        let len = (desc.length as usize).min(target.len());
        let buf = &self.rx_bufs[slot];
        unsafe {
            core::ptr::copy_nonoverlapping(buf.virt() as *const u8, target.as_mut_ptr(), len);
        }

        rings::write_rx_desc(
            &self.rx_ring,
            slot,
            RxDesc {
                buffer_addr: self.dma_iovas.rx_bufs[slot],
                status_error: 0,
                length: 0,
                vlan: 0,
            },
        );
        compiler_fence(Ordering::Release);
        self.rx_head = (slot + 1) % RX_SLOTS;
        self.mmio.write_u32(RDT0, slot as u32)?;
        Ok(len)
    }
}
