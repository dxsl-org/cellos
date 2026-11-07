//! Broadcom BCM57xx controller bring-up, APE handshake, and data path.

use crate::ape::{self, with_ape_lock};
use crate::dma_layout::{
    for_each_initial_dma_program, try_init_array, with_authorized_dma_layout, DmaIovas, DmaSlot,
    InitialDmaProgram, RX_SLOTS, TX_SLOTS,
};
use crate::phy;
use crate::regs::*;
use crate::rings::{self, BcmRxDesc, BcmTxDesc};
use core::sync::atomic::{compiler_fence, Ordering};
use ostd::dma::DmaBuf;
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

pub use crate::rings::BUF_SIZE;

const RESET_POLLS: u32 = 1_000_000;

pub struct BcmController {
    mmio: MmioRegion,
    tx_ring: DmaBuf,
    rx_ring: DmaBuf,
    #[allow(dead_code)]
    status_block: DmaBuf,
    tx_bufs: [DmaBuf; TX_SLOTS],
    rx_bufs: [DmaBuf; RX_SLOTS],
    dma_iovas: DmaIovas,
    tx_next: usize,
    rx_head: usize,
    pub mac: [u8; 6],
    pub link_up: bool,
    pub func: u8,
}

unsafe impl Send for BcmController {}

impl Drop for BcmController {
    fn drop(&mut self) {
        // Unload notification to APE processor prevents dropping BMC NC-SI link.
        let _ = ape::notify_driver_state(&self.mmio, self.func, ape::APE_HOST_DRVR_STATE_UNLOAD);
    }
}

impl BcmController {
    pub fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        let func = (bdf & 0x07) as u8;
        // 1. Notify APE processor that driver is starting.
        ape::notify_driver_state(&mmio, func, ape::APE_HOST_DRVR_STATE_START)?;

        // 2. Perform GRC core reset under GRC lock.
        with_ape_lock(&mmio, func, ape::APE_LOCK_GRC, || {
            let mut val = mmio.read_u32(MISC_CFG)?;
            val |= MISCCFG_RESET_CORE_CLOCKS;
            mmio.write_u32(MISC_CFG, val)?;

            for _ in 0..RESET_POLLS {
                if mmio.read_u32(MISC_CFG)? & MISCCFG_RESET_CORE_CLOCKS == 0 {
                    return Ok(());
                }
                core::hint::spin_loop();
            }
            Err(ViError::IO)
        })?;

        // 3. Read station MAC address from MAC_ADDR1 registers.
        let lo = mmio.read_u32(MAC_ADDR1_LO)?;
        let hi = mmio.read_u32(MAC_ADDR1_HI)?;
        let mac = [
            (lo >> 24) as u8,
            (lo >> 16) as u8,
            (lo >> 8) as u8,
            lo as u8,
            (hi >> 8) as u8,
            hi as u8,
        ];
        if mac == [0u8; 6] {
            return Err(ViError::IO);
        }

        // 4. Restart PHY autonegotiation under PHY lock.
        phy::restart_autoneg(&mmio, func)?;

        // 5. Allocate and authorize DMA.
        let tx_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
        let tx_bufs = try_init_array(|_| DmaBuf::alloc(1).ok_or(ViError::OutOfMemory))?;
        let rx_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
        let rx_bufs = try_init_array(|_| DmaBuf::alloc(1).ok_or(ViError::OutOfMemory))?;
        let status_block = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;

        with_authorized_dma_layout(
            (mmio, tx_ring, rx_ring, status_block, tx_bufs, rx_bufs),
            |r, s| {
                match s {
                    DmaSlot::TxRing => r.1.authorize(bdf),
                    DmaSlot::RxRing => r.2.authorize(bdf),
                    DmaSlot::StatusBlock => r.3.authorize(bdf),
                    DmaSlot::TxBuffer(i) => r.4[i].authorize(bdf),
                    DmaSlot::RxBuffer(i) => r.5[i].authorize(bdf),
                }
                .map_err(|_| ViError::PermissionDenied)
            },
            |(mmio, tx_ring, rx_ring, status_block, tx_bufs, rx_bufs), dma_iovas| {
                rings::clear(&tx_ring);
                rings::clear(&rx_ring);
                rings::clear(&status_block);
                let mut ctrl = BcmController {
                    mmio,
                    tx_ring,
                    rx_ring,
                    status_block,
                    tx_bufs,
                    rx_bufs,
                    dma_iovas,
                    tx_next: 0,
                    rx_head: 0,
                    mac,
                    link_up: false,
                    func,
                };
                for_each_initial_dma_program(&dma_iovas, |p| {
                    let _ = ctrl.apply(p);
                });
                ctrl.finish_init();
                Ok(ctrl)
            },
        )?
    }

    fn apply(&mut self, program: InitialDmaProgram) -> ViResult<()> {
        match program {
            InitialDmaProgram::TxRingBase(_) => {}
            InitialDmaProgram::RxRingBase(_) => {}
            InitialDmaProgram::StatusBlockBase(_) => {}
            InitialDmaProgram::RxBuffer { index, iova } => {
                rings::write_rx_desc(
                    &self.rx_ring,
                    index,
                    BcmRxDesc {
                        addr_hi: (iova >> 32) as u32,
                        addr_lo: iova as u32,
                        idx_len: (index as u32) << 16 | (BUF_SIZE as u32),
                        type_flags: 0,
                    },
                );
            }
        }
        Ok(())
    }

    fn finish_init(&mut self) {
        let _ = self.mmio.write_u32(RX_STD_PRODIDX_REG, RX_SLOTS as u32);
        self.link_up = true;
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
            BcmTxDesc {
                addr_hi: (iova >> 32) as u32,
                addr_lo: iova as u32,
                len_flags: ((frame.len() as u32) << 16)
                    | rings::BCMD_FLAGS_END
                    | rings::BCMD_FLAGS_PACKET_END,
                vlan_tag: 0,
            },
        );
        compiler_fence(Ordering::Release);

        self.tx_next = (slot + 1) % TX_SLOTS;
        self.mmio.write_u32(TX_PRODIDX_REG, self.tx_next as u32)?;

        compiler_fence(Ordering::Acquire);
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
        self.mmio
            .write_u32(RX_STD_PRODIDX_REG, self.rx_head as u32)?;
        Ok(len)
    }
}
