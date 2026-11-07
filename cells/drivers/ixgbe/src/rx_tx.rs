//! Transmit and receive frame operations for Intel ixgbe.

use crate::controller::{IxgbeController, BUF_SIZE};
use crate::dma_layout::{RX_SLOTS, TX_SLOTS};
use crate::regs::{RDT0, TDT0};
use crate::rings::{self, RxDesc, TxDesc};
use core::sync::atomic::{compiler_fence, Ordering};
use types::{ViError, ViResult};

const TX_POLLS: u32 = 1_000_000;

impl IxgbeController {
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
