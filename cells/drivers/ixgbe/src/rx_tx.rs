//! Transmit and receive frame operations for Intel ixgbe.

use crate::controller::IxgbeController;
use crate::regs::{RDT0, TDT0};
use crate::rings::{self, RxDesc, TxDesc, BUF_SIZE, RX_SLOTS, TX_SLOTS};
use core::sync::atomic::{compiler_fence, Ordering};
use types::{ViError, ViResult};

const TX_POLLS: u32 = 1_000_000;

impl IxgbeController {
    pub(crate) fn send_frame(&mut self, frame: &[u8]) -> ViResult<()> {
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

    pub(crate) fn poll_rx(&mut self, target: &mut [u8]) -> ViResult<usize> {
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
