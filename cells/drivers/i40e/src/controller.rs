//! Intel X710/XL710 controller initialization and Tx/Rx data path.

use crate::aq::{self, post_aq_command};
use crate::dma_layout::{
    for_each_initial_dma_program, try_init_array, with_authorized_dma_layout, DmaIovas, DmaSlot,
    InitialDmaProgram, AQ_SLOTS, RX_SLOTS, TX_SLOTS,
};
use crate::regs::*;
use crate::rings::{self, RxDesc, TxDesc};
use core::sync::atomic::{compiler_fence, Ordering};
use ostd::dma::DmaBuf;
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

pub use crate::rings::BUF_SIZE;

const RESET_POLLS: u32 = 1_000_000;

#[allow(dead_code)]
pub struct I40eController {
    pub(crate) mmio: MmioRegion,
    pub(crate) atq_ring: DmaBuf,
    pub(crate) arq_ring: DmaBuf,
    pub(crate) aq_buffer: DmaBuf,
    pub(crate) tx_ring: DmaBuf,
    pub(crate) rx_ring: DmaBuf,
    pub(crate) tx_bufs: [DmaBuf; TX_SLOTS],
    pub(crate) rx_bufs: [DmaBuf; RX_SLOTS],
    pub(crate) dma_iovas: DmaIovas,
    pub(crate) tx_next: usize,
    pub(crate) rx_head: usize,
    pub(crate) atq_head: usize,
    pub mac: [u8; 6],
    pub link_up: bool,
}

unsafe impl Send for I40eController {}

impl I40eController {
    pub fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        // 1. Quiesce & reset controller.
        mmio.write_u32(GLGEN_RST, GLGEN_RST_DEV_ARM)?;
        for _ in 0..RESET_POLLS {
            if mmio.read_u32(GLGEN_RST)? & GLGEN_RST_DEV_ARM == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        // 2. Allocate DMA objects.
        let atq_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
        let arq_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
        let aq_buffer = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
        let tx_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
        let tx_bufs = try_init_array(|_| DmaBuf::alloc(1).ok_or(ViError::OutOfMemory))?;
        let rx_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
        let rx_bufs = try_init_array(|_| DmaBuf::alloc(1).ok_or(ViError::OutOfMemory))?;

        with_authorized_dma_layout(
            (
                mmio, atq_ring, arq_ring, aq_buffer, tx_ring, rx_ring, tx_bufs, rx_bufs,
            ),
            |r, s| {
                match s {
                    DmaSlot::AtqRing => r.1.authorize(bdf),
                    DmaSlot::ArqRing => r.2.authorize(bdf),
                    DmaSlot::AqBuffer => r.3.authorize(bdf),
                    DmaSlot::TxRing => r.4.authorize(bdf),
                    DmaSlot::RxRing => r.5.authorize(bdf),
                    DmaSlot::TxBuffer(i) => r.6[i].authorize(bdf),
                    DmaSlot::RxBuffer(i) => r.7[i].authorize(bdf),
                }
                .map_err(|_| ViError::PermissionDenied)
            },
            |(mmio, atq_ring, arq_ring, aq_buffer, tx_ring, rx_ring, tx_bufs, rx_bufs),
             dma_iovas| {
                rings::clear(&atq_ring);
                rings::clear(&arq_ring);
                rings::clear(&aq_buffer);
                rings::clear(&tx_ring);
                rings::clear(&rx_ring);

                let mut ctrl = I40eController {
                    mmio,
                    atq_ring,
                    arq_ring,
                    aq_buffer,
                    tx_ring,
                    rx_ring,
                    tx_bufs,
                    rx_bufs,
                    dma_iovas,
                    tx_next: 0,
                    rx_head: 0,
                    atq_head: 0,
                    mac: [0; 6],
                    link_up: false,
                };
                for_each_initial_dma_program(&dma_iovas, |p| {
                    let _ = ctrl.apply(p);
                });
                ctrl.init_admin_queue()?;
                ctrl.query_mac_and_link()?;
                Ok(ctrl)
            },
        )?
    }

    fn apply(&mut self, program: InitialDmaProgram) -> ViResult<()> {
        match program {
            InitialDmaProgram::AtqBase(iova) => {
                self.mmio.write_u32(PF_ATQBAL, iova as u32)?;
                self.mmio.write_u32(PF_ATQBAH, (iova >> 32) as u32)?;
                self.mmio
                    .write_u32(PF_ATQLEN, (AQ_SLOTS as u32) | PF_ATQLEN_ENABLE)?;
                self.mmio.write_u32(PF_ATQH, 0)?;
                self.mmio.write_u32(PF_ATQT, 0)?;
            }
            InitialDmaProgram::ArqBase(iova) => {
                self.mmio.write_u32(PF_ARQBAL, iova as u32)?;
                self.mmio.write_u32(PF_ARQBAH, (iova >> 32) as u32)?;
                self.mmio
                    .write_u32(PF_ARQLEN, (AQ_SLOTS as u32) | PF_ARQLEN_ENABLE)?;
                self.mmio.write_u32(PF_ARQH, 0)?;
                self.mmio.write_u32(PF_ARQT, 0)?;
            }
            InitialDmaProgram::TxRingBase(_) => {}
            InitialDmaProgram::RxRingBase(_) => {}
            InitialDmaProgram::RxBuffer { index, iova } => {
                rings::write_rx_desc(
                    &self.rx_ring,
                    index,
                    RxDesc {
                        pkt_addr: iova,
                        hdr_addr: 0,
                    },
                );
            }
        }
        Ok(())
    }

    fn init_admin_queue(&mut self) -> ViResult<()> {
        let cmd = aq::format_get_version();
        let resp = post_aq_command(&self.mmio, &self.atq_ring, self.atq_head, cmd)?;
        self.atq_head = (self.atq_head + 1) % AQ_SLOTS;
        // Verify API major version is 1.
        let api_major = (resp.addr_high >> 16) as u16;
        if api_major != 1 && api_major != 0 {
            return Err(ViError::NotSupported);
        }
        Ok(())
    }

    fn query_mac_and_link(&mut self) -> ViResult<()> {
        let iova = self.dma_iovas.aq_buffer;
        let cmd = aq::format_mac_read(iova, 24);
        let _ = post_aq_command(&self.mmio, &self.atq_ring, self.atq_head, cmd)?;
        self.atq_head = (self.atq_head + 1) % AQ_SLOTS;

        let ptr = self.aq_buffer.virt() as *const u8;
        let mut mac = [0u8; 6];
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, mac.as_mut_ptr(), 6);
        }
        if mac == [0u8; 6] {
            return Err(ViError::IO);
        }
        self.mac = mac;

        let cmd = aq::format_get_link_status();
        let resp = post_aq_command(&self.mmio, &self.atq_ring, self.atq_head, cmd)?;
        self.atq_head = (self.atq_head + 1) % AQ_SLOTS;
        self.link_up = resp.param0 & 1 != 0;
        Ok(())
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
                cmd_type_offset_bsz: (rings::I40E_TXD_QW1_CMD_EOP | rings::I40E_TXD_QW1_CMD_RS)
                    | ((frame.len() as u64) << 34),
            },
        );
        compiler_fence(Ordering::Release);
        self.tx_next = (slot + 1) % TX_SLOTS;
        self.mmio.write_u32(QTX_TAIL, self.tx_next as u32)?;
        Ok(())
    }

    pub fn poll_rx(&mut self, target: &mut [u8]) -> ViResult<usize> {
        let slot = self.rx_head;
        let buf = &self.rx_bufs[slot];
        let len = BUF_SIZE.min(target.len());
        unsafe {
            core::ptr::copy_nonoverlapping(buf.virt() as *const u8, target.as_mut_ptr(), len);
        }
        self.rx_head = (slot + 1) % RX_SLOTS;
        self.mmio.write_u32(QRX_TAIL, self.rx_head as u32)?;
        Ok(len)
    }
}
