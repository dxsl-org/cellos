//! DMA layout, generation-bit rings and descriptor encoding (pure; host-tested).
//!
//! Every vmxnet3 ring is owned by whichever side's generation bit a slot
//! carries: a producer writes a slot with its current generation and flips the
//! generation each time its index wraps, and a consumer accepts a slot only
//! while the slot's bit equals its own generation. Both sides start at 1, so a
//! zeroed ring is empty for the device's command rings and free for its
//! completion rings. Descriptor bit positions follow FreeBSD's `if_vmxreg.h`
//! (`vmxnet3_txdesc`, `vmxnet3_rxdesc`, `vmxnet3_txcompdesc`,
//! `vmxnet3_rxcompdesc`; ISC, see `NOTICE`).

use crate::shared::{Sink, DRIVER_SHARED_LEN, QUEUES_LEN};
use core::sync::atomic::{fence, Ordering};

pub(crate) const TX_SLOTS: usize = 32;
pub(crate) const RX_SLOTS: usize = 32;
pub(crate) const RX_COMP_SLOTS: usize = 2 * RX_SLOTS;
/// Command and completion descriptors are 16 bytes; TX data-ring entries 128.
const DESC_LEN: usize = 16;
const TX_DATA_DESC_LEN: usize = 128;
/// Frame buffer per slot (one Ethernet frame without FCS fits).
pub const BUF_SIZE: usize = 2048;

// ── Layout of the single DMA area ────────────────────────────────────────────

pub(crate) const SHARED_OFF: usize = 0;
pub(crate) const QUEUES_OFF: usize = 1024;
pub(crate) const TX_RING_OFF: usize = 4096;
pub(crate) const TX_COMP_OFF: usize = TX_RING_OFF + TX_SLOTS * DESC_LEN;
pub(crate) const RX_RING_OFF: [usize; 2] = [TX_COMP_OFF + TX_SLOTS * DESC_LEN, RX_RING_OFF_1];
const RX_RING_OFF_1: usize = TX_COMP_OFF + (TX_SLOTS + RX_SLOTS) * DESC_LEN;
pub(crate) const RX_COMP_OFF: usize = RX_RING_OFF_1 + RX_SLOTS * DESC_LEN;
pub(crate) const TX_DATA_OFF: usize = 2 * 4096;
pub(crate) const TX_BUF_OFF: usize = TX_DATA_OFF + TX_SLOTS * TX_DATA_DESC_LEN;
pub(crate) const RX_BUF_OFF: usize = TX_BUF_OFF + TX_SLOTS * BUF_SIZE;
pub(crate) const DMA_PAGES: usize = (RX_BUF_OFF + RX_SLOTS * BUF_SIZE) / 4096;

const _: () = assert!(SHARED_OFF + DRIVER_SHARED_LEN <= QUEUES_OFF);
const _: () = assert!(QUEUES_OFF.is_multiple_of(128) && QUEUES_OFF + QUEUES_LEN <= TX_RING_OFF);
const _: () = assert!(RX_COMP_OFF + RX_COMP_SLOTS * DESC_LEN <= TX_DATA_OFF);
const _: () = assert!(
    TX_COMP_OFF.is_multiple_of(512)
        && RX_RING_OFF_1.is_multiple_of(512)
        && RX_COMP_OFF.is_multiple_of(512)
);
const _: () = assert!((RX_BUF_OFF + RX_SLOTS * BUF_SIZE).is_multiple_of(4096));

// ── Descriptor bits ──────────────────────────────────────────────────────────

const LEN_MASK: u32 = 0x3FFF;
const TXD_GEN: u32 = 1 << 14;
const TXD_EOP: u32 = 1 << 12;
const TXD_CQ: u32 = 1 << 13;
const RXD_GEN: u32 = 1 << 31;
const COMP_GEN: u32 = 1 << 31;
const RXCD_EOP: u32 = 1 << 14;
const RXCD_SOP: u32 = 1 << 15;
const RXCD_ERR: u32 = 1 << 14;
const IDX_MASK: u32 = 0xFFF;

/// One side's position in a ring: next slot and current generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GenRing {
    pub next: usize,
    pub gen: bool,
    size: usize,
}

impl GenRing {
    pub(crate) const fn new(size: usize) -> Self {
        Self {
            next: 0,
            gen: true,
            size,
        }
    }

    /// Step past the current slot, flipping the generation on wrap.
    pub(crate) fn advance(&mut self) {
        self.next += 1;
        if self.next == self.size {
            self.next = 0;
            self.gen = !self.gen;
        }
    }
}

/// Write TX descriptor `slot` for one `len`-byte frame at `addr` with
/// generation `gen`; the generation word goes last, after a release fence.
pub(crate) fn post_tx(s: &mut impl Sink, slot: usize, addr: u64, len: u16, gen: bool) {
    let desc = TX_RING_OFF + slot % TX_SLOTS * DESC_LEN;
    s.put64(desc, addr);
    s.put32(desc + 12, TXD_EOP | TXD_CQ);
    fence(Ordering::Release);
    s.put32(
        desc + 8,
        (u32::from(len) & LEN_MASK) | if gen { TXD_GEN } else { 0 },
    );
}

/// Write RX ring-0 descriptor `slot` as a head buffer of [`BUF_SIZE`] bytes.
pub(crate) fn post_rx(s: &mut impl Sink, slot: usize, addr: u64, gen: bool) {
    let desc = RX_RING_OFF[0] + slot % RX_SLOTS * DESC_LEN;
    s.put64(desc, addr);
    s.put32(desc + 12, 0);
    fence(Ordering::Release);
    s.put32(
        desc + 8,
        BUF_SIZE as u32 & LEN_MASK | if gen { RXD_GEN } else { 0 },
    );
}

/// The TX ring index of the completion at `ring.next`, once the device wrote
/// it with the consumer's generation.
pub(crate) fn tx_completion(read32: impl Fn(usize) -> u32, ring: &GenRing) -> Option<usize> {
    let desc = TX_COMP_OFF + ring.next * DESC_LEN;
    if (read32(desc + 12) & COMP_GEN != 0) != ring.gen {
        return None;
    }
    fence(Ordering::Acquire);
    Some((read32(desc) & IDX_MASK) as usize)
}

/// A received-frame completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RxCompletion {
    pub index: usize,
    /// 0 = ring 0 (head buffers), 1 = ring 1 (body buffers).
    pub ring: u32,
    pub sop: bool,
    pub eop: bool,
    pub error: bool,
    pub len: usize,
}

/// The RX completion at `ring.next`, once written with the consumer's
/// generation.
pub(crate) fn rx_completion(read32: impl Fn(usize) -> u32, ring: &GenRing) -> Option<RxCompletion> {
    let desc = RX_COMP_OFF + ring.next * DESC_LEN;
    if (read32(desc + 12) & COMP_GEN != 0) != ring.gen {
        return None;
    }
    fence(Ordering::Acquire);
    let (word0, word2) = (read32(desc), read32(desc + 8));
    Some(RxCompletion {
        index: (word0 & IDX_MASK) as usize,
        ring: (word0 >> 16) & 0x3FF,
        sop: word0 & RXCD_SOP != 0,
        eop: word0 & RXCD_EOP != 0,
        error: word2 & RXCD_ERR != 0,
        len: (word2 & LEN_MASK) as usize,
    })
}

#[cfg(test)]
#[path = "rings_tests.rs"]
mod tests;
