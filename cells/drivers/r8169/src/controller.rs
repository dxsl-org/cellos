//! Realtek RTL8168/8125 controller bring-up and Tx/Rx data path.

use crate::dma_layout::{
    for_each_initial_dma_program, try_init_array, with_authorized_dma_layout, DmaIovas, DmaSlot,
    InitialDmaProgram, RX_SLOTS, TX_SLOTS,
};
use crate::identity::{decode_xid, RtlXid};
use crate::phy;
use crate::regs::*;
use crate::rings::{self, RtlDesc};
use core::sync::atomic::{compiler_fence, Ordering};
use ostd::dma::DmaBuf;
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

pub use crate::rings::BUF_SIZE;

const RESET_POLLS: u32 = 1_000_000;
const TX_POLLS: u32 = 1_000_000;
const LINK_POLLS: u32 = 6_000_000;

pub struct RtlController {
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
    #[allow(dead_code)]
    pub xid: RtlXid,
}

unsafe impl Send for RtlController {}

impl RtlController {
    pub fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
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

        // 4. Allocate and authorize DMA.
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
                let mut ctrl = RtlController {
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
                    xid,
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
                self.mmio.write_u32(TXLIST_ADDR_LO, iova as u32)?;
                self.mmio.write_u32(TXLIST_ADDR_HI, (iova >> 32) as u32)?;
            }
            InitialDmaProgram::RxRingBase(iova) => {
                self.mmio.write_u32(RXLIST_ADDR_LO, iova as u32)?;
                self.mmio.write_u32(RXLIST_ADDR_HI, (iova >> 32) as u32)?;
            }
            InitialDmaProgram::RxBuffer { index, iova } => {
                let mut opts = rings::DESC_OWN | (BUF_SIZE as u32);
                if index == RX_SLOTS - 1 {
                    opts |= rings::DESC_RING_END;
                }
                rings::write_desc(
                    &self.rx_ring,
                    index,
                    RtlDesc {
                        opts1: opts,
                        opts2: 0,
                        addr_lo: iova as u32,
                        addr_hi: (iova >> 32) as u32,
                    },
                );
            }
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
        let mut opts = rings::DESC_OWN | rings::DESC_FS | rings::DESC_LS | (frame.len() as u32);
        if slot == TX_SLOTS - 1 {
            opts |= rings::DESC_RING_END;
        }
        rings::write_desc(
            &self.tx_ring,
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
            if rings::read_desc(&self.tx_ring, slot).opts1 & rings::DESC_OWN == 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(ViError::IO)
    }

    pub fn poll_rx(&mut self, target: &mut [u8]) -> ViResult<usize> {
        let slot = self.rx_head;
        compiler_fence(Ordering::Acquire);
        let desc = rings::read_desc(&self.rx_ring, slot);
        if desc.opts1 & rings::DESC_OWN != 0 {
            return Ok(0);
        }
        let len = ((desc.opts1 & 0x3FFF) as usize).min(target.len());
        let buf = &self.rx_bufs[slot];
        unsafe {
            core::ptr::copy_nonoverlapping(buf.virt() as *const u8, target.as_mut_ptr(), len);
        }

        let mut opts = rings::DESC_OWN | (BUF_SIZE as u32);
        if slot == RX_SLOTS - 1 {
            opts |= rings::DESC_RING_END;
        }
        rings::write_desc(
            &self.rx_ring,
            slot,
            RtlDesc {
                opts1: opts,
                opts2: 0,
                addr_lo: self.dma_iovas.rx_bufs[slot] as u32,
                addr_hi: (self.dma_iovas.rx_bufs[slot] >> 32) as u32,
            },
        );
        compiler_fence(Ordering::Release);
        self.rx_head = (slot + 1) % RX_SLOTS;
        Ok(len)
    }
}
