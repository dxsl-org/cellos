//! Intel 82574L bring-up and the polled-RX / interrupt-assisted-TX data path.
//!
//! Order (82574 datasheet §4.6 initialization): quiesce bus mastering, global
//! reset, mask interrupts, station address (`RAL0`/`RAH0` auto-loaded from NVM,
//! `EERD` words 0..3 as the fallback), interrupt routing, PHY autoneg restart,
//! DMA authorization, rings, TX enable, bounded link wait, RX enable. No
//! device-visible address is programmed before every DMA object is authorized.

use crate::dma_layout::{
    for_each_initial_dma_program, try_init_array, with_authorized_dma_layout, DmaIovas, DmaSlot,
    InitialDmaProgram, RX_SLOTS, TX_SLOTS,
};
use crate::irq::NicIrq;
use crate::regs::*;
use crate::reset::reset_and_read_mac;
use crate::{phy, rings};
use core::sync::atomic::{compiler_fence, Ordering};
use ostd::dma::DmaBuf;
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

pub use crate::rings::BUF_SIZE;

/// Link wait: up to 50 parks (~5 s) with MSI-X, or this many spins when polled
/// (the igb lane's budget, which covers QEMU's ~500 ms autoneg timer).
const LINK_PARKS: u32 = 50;
const LINK_POLLS: u32 = 6_000_000;
/// TX completion: up to 20 parks (~2 s) with MSI-X, or this many spins.
const TX_PARKS: u32 = 20;
const TX_POLLS: u32 = 1_000_000;

pub struct E1000eController {
    mmio: MmioRegion,
    irq: NicIrq,
    tx_ring: DmaBuf,
    rx_ring: DmaBuf,
    tx_bufs: [DmaBuf; TX_SLOTS],
    rx_bufs: [DmaBuf; RX_SLOTS],
    dma_iovas: DmaIovas,
    tx_next: usize,
    rx_head: usize,
    /// Dropping the descriptors of a frame spread over several of them.
    rx_in_fragment: bool,
    pub mac: [u8; 6],
    /// Whether `STATUS.LU` was up before RX was enabled.
    pub link_up: bool,
}

impl E1000eController {
    /// Bring the controller up on `mmio` (BAR0) for the function at `bdf`.
    pub fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        let mac = reset_and_read_mac(&mmio)?;

        let irq = NicIrq::setup(bdf);
        irq.program(&mmio)?;
        mmio.wr32(
            CTRL,
            (mmio.rd32(CTRL)? | CTRL_SLU | CTRL_ASDE) & !CTRL_GIO_MASTER_DISABLE,
        )?;
        phy::restart_autoneg(&mmio)?;

        let tx_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
        let tx_bufs = try_init_array(|_| DmaBuf::alloc(1).ok_or(ViError::OutOfMemory))?;
        let rx_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
        let rx_bufs = try_init_array(|_| DmaBuf::alloc(1).ok_or(ViError::OutOfMemory))?;

        with_authorized_dma_layout(
            (mmio, tx_ring, rx_ring, tx_bufs, rx_bufs),
            |resources, slot| {
                let result = match slot {
                    DmaSlot::TxRing => resources.1.authorize(bdf),
                    DmaSlot::TxBuffer(index) => resources.3[index].authorize(bdf),
                    DmaSlot::RxRing => resources.2.authorize(bdf),
                    DmaSlot::RxBuffer(index) => resources.4[index].authorize(bdf),
                };
                result.map_err(|_| ViError::PermissionDenied)
            },
            |(mmio, tx_ring, rx_ring, tx_bufs, rx_bufs), dma_iovas| {
                rings::clear(&tx_ring);
                rings::clear(&rx_ring);
                let mut ctrl = E1000eController {
                    mmio,
                    irq,
                    tx_ring,
                    rx_ring,
                    tx_bufs,
                    rx_bufs,
                    dma_iovas,
                    tx_next: 0,
                    rx_head: 0,
                    rx_in_fragment: false,
                    mac,
                    link_up: false,
                };
                let mut result = Ok(());
                for_each_initial_dma_program(&dma_iovas, |program| {
                    if result.is_ok() {
                        result = ctrl.apply(program);
                    }
                });
                result.map(|()| ctrl)
            },
        )?
    }

    fn apply(&mut self, program: InitialDmaProgram) -> ViResult<()> {
        let m = &self.mmio;
        match program {
            InitialDmaProgram::TxRingBase(iova) => {
                m.wr32(TDBAL, iova as u32)?;
                m.wr32(TDBAH, (iova >> 32) as u32)?;
                m.wr32(TDLEN, rings::ring_bytes(TX_SLOTS))?;
                m.wr32(TDH, 0)?;
                m.wr32(TDT, 0)
            }
            InitialDmaProgram::RxDescriptor { slot, iova } => {
                rings::post_rx(&self.rx_ring, slot, iova);
                Ok(())
            }
            InitialDmaProgram::RxRingBase(iova) => {
                m.wr32(RDBAL, iova as u32)?;
                m.wr32(RDBAH, (iova >> 32) as u32)?;
                m.wr32(RDLEN, rings::ring_bytes(RX_SLOTS))?;
                m.wr32(RDH, 0)?;
                m.wr32(RDT, (RX_SLOTS - 1) as u32)
            }
            InitialDmaProgram::Enable => {
                m.wr32(TIPG, TIPG_COPPER)?;
                m.wr32(TCTL, TCTL_EN | TCTL_PSP | TCTL_CT | TCTL_COLD)?;
                // The receiver accepts frames only with link up, so wait for
                // autoneg (bounded) before enabling it; a timeout is reported
                // in the bind line, not fatal.
                let mmio = &self.mmio;
                self.link_up = self.irq.wait_for(mmio, LINK_PARKS, LINK_POLLS, || {
                    Ok(mmio.rd32(STATUS)? & STATUS_LU != 0)
                })?;
                m.wr32(RCTL, RCTL_EN | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_SECRC)
            }
        }
    }

    /// MSI-X entries held (0 when polled), why the cell is polled, and how many
    /// parks so far ended by an interrupt / by their deadline.
    pub fn irq_state(&self) -> (u8, &'static str, u32, u32) {
        let irq = &self.irq;
        (irq.vectors(), irq.reason, irq.wakes, irq.expiries)
    }

    /// Transmit `frame` (no FCS) and wait, bounded, for its write-back.
    /// `IO` when the slot to fill was posted and never written back: the
    /// controller may still read it, so it is not overwritten.
    pub fn send_frame(&mut self, frame: &[u8]) -> ViResult<()> {
        if frame.is_empty() || frame.len() > BUF_SIZE {
            return Err(ViError::InvalidInput);
        }
        let slot = self.tx_next;
        if !rings::tx_slot_free(&self.tx_ring, slot) {
            return Err(ViError::IO);
        }
        self.tx_next = (slot + 1) % TX_SLOTS;
        rings::copy_in(&self.tx_bufs[slot], frame);
        let iova = self.dma_iovas.tx_descriptor_iova(slot);
        rings::post_tx(&self.tx_ring, slot, iova, frame.len() as u16);
        compiler_fence(Ordering::Release);
        self.mmio.wr32(TDT, self.tx_next as u32)?;

        let ring = &self.tx_ring;
        if self.irq.wait_for(&self.mmio, TX_PARKS, TX_POLLS, || {
            Ok(rings::tx_done(ring, slot))
        })? {
            Ok(())
        } else {
            Err(ViError::IO)
        }
    }

    /// Take the next received frame into `out`; 0 when none is ready.
    /// Errored and multi-descriptor frames are dropped.
    pub fn recv_frame(&mut self, out: &mut [u8]) -> usize {
        for _ in 0..RX_SLOTS {
            let head = self.rx_head;
            let length = match rings::rx_completed(&self.rx_ring, head, &mut self.rx_in_fragment) {
                rings::RxCompletion::Pending => return 0,
                rings::RxCompletion::Frame(length) => Some(length.min(out.len()).min(BUF_SIZE)),
                rings::RxCompletion::Drop => None,
            };
            if let Some(copy_len) = length {
                rings::copy_out(&self.rx_bufs[head], out, copy_len);
            }
            rings::post_rx(&self.rx_ring, head, self.dma_iovas.rx_descriptor_iova(head));
            self.rx_head = (head + 1) % RX_SLOTS;
            compiler_fence(Ordering::Release);
            let _ = self.mmio.wr32(RDT, head as u32);
            if let Some(copy_len) = length {
                return copy_len;
            }
        }
        0
    }
}
