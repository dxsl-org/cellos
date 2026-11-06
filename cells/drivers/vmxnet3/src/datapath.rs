//! The data path: interrupt-assisted TX with a bounded completion wait, and
//! request-driven RX (the net service asks; nothing here waits for a frame).

use crate::controller::Vmxnet3;
use crate::dma::DmaArea;
use crate::regs::*;
use crate::rings::*;
use crate::shared::{EVENT_OFF, RXQ_UPDATE_RXHEAD_OFF};
use core::sync::atomic::{fence, Ordering};
use ostd::io::print_fmt;
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

/// TX completion: up to 20 parks (~2 s) with MSI-X, or this many polls.
const TX_PARKS: u32 = 20;
const TX_POLLS: u32 = 1_000_000;

impl Vmxnet3 {
    /// Transmit `frame` (no FCS) and wait, bounded, for its completion.
    pub fn send_frame(&mut self, frame: &[u8]) -> ViResult<()> {
        if frame.is_empty() || frame.len() > BUF_SIZE {
            return Err(ViError::InvalidInput);
        }
        reap_tx(&self.dma, &mut self.tx_comp, &mut self.tx_clean);
        let slot = self.tx.next;
        if (slot + 1) % TX_SLOTS == self.tx_clean {
            return Err(ViError::IO); // ring full: earlier frames never completed
        }
        let buffer = TX_BUF_OFF + slot * BUF_SIZE;
        self.dma.copy_in(buffer, frame);
        let addr = self.dma.iova(buffer);
        post_tx(&mut self.dma, slot, addr, frame.len() as u16, self.tx.gen);
        self.tx.advance();
        fence(Ordering::SeqCst);
        self.pt.wr32(PT_TXPROD, self.tx.next as u32)?;

        let (dma, comp, clean, next) = (
            &self.dma,
            &mut self.tx_comp,
            &mut self.tx_clean,
            self.tx.next,
        );
        if self.irq.wait_for(TX_PARKS, TX_POLLS, || {
            reap_tx(dma, comp, clean);
            Ok(*clean == next)
        })? {
            Ok(())
        } else {
            Err(ViError::IO)
        }
    }

    /// Take the next received frame into `out`; 0 when none is ready.
    /// Fragmented, errored or oversized frames are dropped.
    pub fn recv_frame(&mut self, out: &mut [u8]) -> usize {
        loop {
            let dma = &self.dma;
            let Some(done) = rx_completion(|o| dma.read32(o), &self.rx_comp) else {
                return 0;
            };
            self.rx_comp.advance();
            if done.ring != 0 || done.index >= RX_SLOTS {
                continue; // nothing this cell posted
            }
            let good = done.sop && done.eop && !done.error && done.len > 0 && done.len <= BUF_SIZE;
            let len = if good { done.len.min(out.len()) } else { 0 };
            self.dma
                .copy_out(RX_BUF_OFF + done.index * BUF_SIZE, &mut out[..len]);
            self.refill();
            if good {
                return len;
            }
        }
    }

    /// Hand the empty ring-0 slot back to the device with its own buffer.
    fn refill(&mut self) {
        let slot = self.rx_fill.next;
        let addr = self.dma.iova(RX_BUF_OFF + slot * BUF_SIZE);
        post_rx(&mut self.dma, slot, addr, self.rx_fill.gen);
        self.rx_fill.advance();
        if self.dma.read8(QUEUES_OFF + RXQ_UPDATE_RXHEAD_OFF) != 0 {
            fence(Ordering::SeqCst);
            let _ = self.pt.wr32(PT_RXPROD, self.rx_fill.next as u32);
        }
    }
}

/// Consume every TX completion the device wrote.
fn reap_tx(dma: &DmaArea, comp: &mut GenRing, clean: &mut usize) {
    while let Some(index) = tx_completion(|o| dma.read32(o), comp) {
        comp.advance();
        if index < TX_SLOTS {
            *clean = (index + 1) % TX_SLOTS;
        }
    }
}

/// Acknowledge pending device events; queue errors are logged.
pub(crate) fn ack_events(vd: &MmioRegion, dma: &DmaArea) -> ViResult<()> {
    let events = dma.read32(SHARED_OFF + EVENT_OFF);
    if events != 0 {
        vd.wr32(VD_ECR, events)?;
        if events & (EVENT_RQERROR | EVENT_TQERROR) != 0 {
            let _ = print_fmt(format_args!(
                "[vmxnet3] device queue error event {events:#x}\n"
            ));
        }
    }
    Ok(())
}
