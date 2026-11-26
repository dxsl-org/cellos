//! A polled split virtqueue over byte-copy DMA access, with fixed-length
//! chains in fixed descriptor slots: slot `s` owns descriptors
//! `s * chain .. (s + 1) * chain` and its head is `s * chain`.
//!
//! The cell has no `unsafe`, so DMA memory is reached only through
//! [`Mem`] (on the cell, `ostd::dma::DmaBuf`'s volatile byte copies). Every
//! multi-byte ring field is therefore stored and loaded a byte at a time:
//! - Used index (device-written): read until two loads agree, so a load that
//!   straddles the device's 16-bit store is never acted on.
//! - Avail index (driver-written): low byte first. When the low byte wraps
//!   (every 256th publication) a device load between the two byte stores sees
//!   an index up to 255 behind; a device that then flags `DEVICE_NEEDS_RESET`
//!   is re-initialised by [`crate::tx`].

use crate::ring::{RingLayout, AVAIL_F_NO_INTERRUPT, DESC_F_NEXT, DESC_F_WRITE};
use core::sync::atomic::{fence, Ordering};

/// Device-visible memory, addressed by offset into the DMA allocation.
/// Offsets come from [`crate::ring`]; an offset outside the allocation is a
/// bug and panics instead of touching foreign memory.
pub trait Mem {
    fn write(&self, off: usize, bytes: &[u8]);
    fn read(&self, off: usize, out: &mut [u8]);
    /// Device-visible address of byte `off`.
    fn iova(&self, off: usize) -> u64;
}

/// One chain segment: DMA offset, length, device-writable.
pub type Segment = (usize, usize, bool);

/// The device returned something it was never given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadCompletion;

pub struct SplitQueue {
    pub index: u16,
    pub ring: RingLayout,
    chain: u16,
    avail_idx: u16,
    last_used: u16,
    /// Bit `s` set while slot `s` is with the device.
    in_flight: u32,
}

impl SplitQueue {
    /// Queue `index` with `size` entries split into `size / chain` slots
    /// (at most 32).
    pub fn new(index: u16, size: u16, chain: u16) -> Self {
        Self {
            index,
            ring: RingLayout::new(index, size),
            chain,
            avail_idx: 0,
            last_used: 0,
            in_flight: 0,
        }
    }

    /// Zeroes the rings and asks the device not to interrupt. The queue
    /// must not be live (before `queue_set`, or after a device reset).
    pub fn reset(&mut self, mem: &impl Mem) {
        let zero = [0u8; 64];
        let mut off = self.ring.desc;
        while off < self.ring.end {
            let len = (self.ring.end - off).min(zero.len());
            mem.write(off, &zero[..len]);
            off += len;
        }
        mem.write(self.ring.avail, &AVAIL_F_NO_INTERRUPT.to_le_bytes());
        self.avail_idx = 0;
        self.last_used = 0;
        self.in_flight = 0;
    }

    /// Device addresses of the descriptor table, avail ring and used ring.
    pub fn areas(&self, mem: &impl Mem) -> (u64, u64, u64) {
        let ring = &self.ring;
        (
            mem.iova(ring.desc),
            mem.iova(ring.avail),
            mem.iova(ring.used),
        )
    }

    pub fn slots(&self) -> u16 {
        (self.ring.size / self.chain).min(32)
    }

    pub fn in_flight(&self) -> u32 {
        self.in_flight.count_ones()
    }

    /// A slot the device does not hold.
    pub fn free_slot(&self) -> Option<u16> {
        (0..self.slots()).find(|&slot| self.in_flight & (1 << slot) == 0)
    }

    /// Publishes `chain` (exactly `self.chain` segments) in free slot `slot`.
    /// The caller notifies the device afterwards.
    pub fn submit(&mut self, mem: &impl Mem, slot: u16, chain: &[Segment]) {
        assert!(chain.len() == usize::from(self.chain) && slot < self.slots());
        assert!(self.in_flight & (1 << slot) == 0);
        let head = slot * self.chain;
        for (i, &(off, len, writable)) in chain.iter().enumerate() {
            let index = head + i as u16;
            let next = i + 1 < chain.len();
            let mut flags = if writable { DESC_F_WRITE } else { 0 };
            if next {
                flags |= DESC_F_NEXT;
            }
            let mut desc = [0u8; 16];
            desc[0..8].copy_from_slice(&mem.iova(off).to_le_bytes());
            desc[8..12].copy_from_slice(&(len as u32).to_le_bytes());
            desc[12..14].copy_from_slice(&flags.to_le_bytes());
            desc[14..16].copy_from_slice(&(if next { index + 1 } else { 0 }).to_le_bytes());
            mem.write(self.ring.desc_entry(index), &desc);
        }
        mem.write(self.ring.avail_entry(self.avail_idx), &head.to_le_bytes());
        self.avail_idx = self.avail_idx.wrapping_add(1);
        self.in_flight |= 1 << slot;
        // Descriptors, buffers and the ring entry before the index.
        fence(Ordering::Release);
        mem.write(self.ring.avail_idx(), &self.avail_idx.to_le_bytes());
        // The index before the notify write.
        fence(Ordering::SeqCst);
    }

    /// The next completion: its slot and the byte count the device wrote.
    pub fn pop_used(&mut self, mem: &impl Mem) -> Result<Option<(u16, u32)>, BadCompletion> {
        let used_idx = stable_u16(mem, self.ring.used_idx());
        if used_idx == self.last_used {
            return Ok(None);
        }
        if used_idx.wrapping_sub(self.last_used) > self.in_flight() as u16 {
            return Err(BadCompletion);
        }
        // The used element and every device write after the index.
        fence(Ordering::Acquire);
        let mut elem = [0u8; 8];
        mem.read(self.ring.used_entry(self.last_used), &mut elem);
        let id = u32::from_le_bytes([elem[0], elem[1], elem[2], elem[3]]);
        let len = u32::from_le_bytes([elem[4], elem[5], elem[6], elem[7]]);
        let slot = id / u32::from(self.chain);
        if id % u32::from(self.chain) != 0 || slot >= 32 || self.in_flight & (1 << slot) == 0 {
            return Err(BadCompletion);
        }
        self.in_flight &= !(1 << slot);
        self.last_used = self.last_used.wrapping_add(1);
        Ok(Some((slot as u16, len)))
    }
}

/// A device-written 16-bit field, read until two byte-wise loads agree.
fn stable_u16(mem: &impl Mem, off: usize) -> u16 {
    let load = || {
        let mut bytes = [0u8; 2];
        mem.read(off, &mut bytes);
        u16::from_le_bytes(bytes)
    };
    let mut last = load();
    loop {
        let next = load();
        if next == last {
            return next;
        }
        last = next;
    }
}
