// SPDX-License-Identifier: Apache-2.0
//! Lock-Free Single-Producer Single-Consumer (SPSC) Ring Channel.
//!
//! Provides zero-trap, zero-allocation, shared-memory IPC between cells
//! residing within the Cellos Single Address Space (SAS).

#![forbid(unsafe_code)]

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Number of bytes in each ring buffer slot payload.
pub const RING_SLOT_BYTES: usize = 64;
/// Number of 64-bit atomic words per slot payload.
pub const RING_WORDS_PER_SLOT: usize = RING_SLOT_BYTES / 8;
/// Maximum number of messages held in one ring buffer (power of two).
pub const RING_CAPACITY: usize = 16;

/// Error conditions for ring channel operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingError {
    /// The ring buffer has no free slots available.
    Full,
    /// The ring buffer has no pending messages.
    Empty,
    /// Message exceeds `RING_SLOT_BYTES`.
    MessageTooLarge,
    /// Output buffer is smaller than the message length.
    BufferTooSmall,
}

/// Metadata associated with a received ring message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RingMessageMeta {
    /// Payload length in bytes.
    pub len: usize,
    /// Monotonic sequence number.
    pub seq: u32,
    /// Optional application flags.
    pub flags: u32,
}

/// A single message slot within the SPSC ring buffer.
pub struct SpscSlot {
    /// Slot status: 0 = EMPTY, 1 = WRITING, 2 = READY
    pub state: AtomicU32,
    /// Number of valid data bytes in this slot.
    pub len: AtomicU32,
    /// Message sequence number.
    pub seq: AtomicU32,
    /// Application flags.
    pub flags: AtomicU32,
    /// Atomic 64-bit payload words.
    pub data: [AtomicU64; RING_WORDS_PER_SLOT],
}

impl SpscSlot {
    pub const STATE_EMPTY: u32 = 0;
    pub const STATE_WRITING: u32 = 1;
    pub const STATE_READY: u32 = 2;

    pub const fn new() -> Self {
        Self {
            state: AtomicU32::new(Self::STATE_EMPTY),
            len: AtomicU32::new(0),
            seq: AtomicU32::new(0),
            flags: AtomicU32::new(0),
            data: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
        }
    }
}

impl Default for SpscSlot {
    fn default() -> Self {
        Self::new()
    }
}

/// A unidirectional Single-Producer Single-Consumer lock-free ring buffer.
pub struct SpscRing {
    /// Sequence counter advanced by the producer.
    pub producer_idx: AtomicU32,
    /// Sequence counter advanced by the consumer.
    pub consumer_idx: AtomicU32,
    /// Fixed-capacity slots array.
    pub slots: [SpscSlot; RING_CAPACITY],
}

impl SpscRing {
    /// Construct a new empty ring buffer.
    pub const fn new() -> Self {
        const EMPTY_SLOT: SpscSlot = SpscSlot::new();
        Self {
            producer_idx: AtomicU32::new(0),
            consumer_idx: AtomicU32::new(0),
            slots: [
                EMPTY_SLOT, EMPTY_SLOT, EMPTY_SLOT, EMPTY_SLOT, EMPTY_SLOT, EMPTY_SLOT, EMPTY_SLOT,
                EMPTY_SLOT, EMPTY_SLOT, EMPTY_SLOT, EMPTY_SLOT, EMPTY_SLOT, EMPTY_SLOT, EMPTY_SLOT,
                EMPTY_SLOT, EMPTY_SLOT,
            ],
        }
    }

    /// Number of queued messages ready to be popped.
    pub fn len(&self) -> usize {
        let p = self.producer_idx.load(Ordering::Relaxed);
        let c = self.consumer_idx.load(Ordering::Relaxed);
        p.wrapping_sub(c) as usize
    }

    /// Check if the ring has no messages.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check if the ring is currently at full capacity.
    pub fn is_full(&self) -> bool {
        self.len() >= RING_CAPACITY
    }

    /// Attempt to enqueue a message without blocking.
    pub fn try_push(&self, msg: &[u8], seq: u32, flags: u32) -> Result<(), RingError> {
        if msg.len() > RING_SLOT_BYTES {
            return Err(RingError::MessageTooLarge);
        }

        let p = self.producer_idx.load(Ordering::Relaxed);
        let c = self.consumer_idx.load(Ordering::Acquire);

        if p.wrapping_sub(c) >= RING_CAPACITY as u32 {
            return Err(RingError::Full);
        }

        let slot_idx = (p as usize) & (RING_CAPACITY - 1);
        let slot = &self.slots[slot_idx];

        if slot.state.load(Ordering::Acquire) != SpscSlot::STATE_EMPTY {
            return Err(RingError::Full);
        }

        slot.state.store(SpscSlot::STATE_WRITING, Ordering::Relaxed);

        let mut words = [0u64; RING_WORDS_PER_SLOT];
        let mut i = 0;
        while i < msg.len() {
            let word_idx = i / 8;
            let byte_offset = i % 8;
            words[word_idx] |= (msg[i] as u64) << (byte_offset * 8);
            i += 1;
        }

        for w in 0..RING_WORDS_PER_SLOT {
            slot.data[w].store(words[w], Ordering::Relaxed);
        }

        slot.len.store(msg.len() as u32, Ordering::Relaxed);
        slot.seq.store(seq, Ordering::Relaxed);
        slot.flags.store(flags, Ordering::Relaxed);

        slot.state.store(SpscSlot::STATE_READY, Ordering::Release);
        self.producer_idx
            .store(p.wrapping_add(1), Ordering::Release);

        Ok(())
    }

    /// Attempt to dequeue a message without blocking.
    pub fn try_pop(&self, out: &mut [u8]) -> Result<Option<RingMessageMeta>, RingError> {
        let c = self.consumer_idx.load(Ordering::Relaxed);
        let p = self.producer_idx.load(Ordering::Acquire);

        if c == p {
            return Ok(None);
        }

        let slot_idx = (c as usize) & (RING_CAPACITY - 1);
        let slot = &self.slots[slot_idx];

        if slot.state.load(Ordering::Acquire) != SpscSlot::STATE_READY {
            return Ok(None);
        }

        let len = slot.len.load(Ordering::Relaxed) as usize;
        if out.len() < len {
            return Err(RingError::BufferTooSmall);
        }

        let seq = slot.seq.load(Ordering::Relaxed);
        let flags = slot.flags.load(Ordering::Relaxed);

        let mut words = [0u64; RING_WORDS_PER_SLOT];
        for w in 0..RING_WORDS_PER_SLOT {
            words[w] = slot.data[w].load(Ordering::Relaxed);
        }

        let mut i = 0;
        while i < len {
            let word_idx = i / 8;
            let byte_offset = i % 8;
            out[i] = ((words[word_idx] >> (byte_offset * 8)) & 0xff) as u8;
            i += 1;
        }

        slot.state.store(SpscSlot::STATE_EMPTY, Ordering::Release);
        self.consumer_idx
            .store(c.wrapping_add(1), Ordering::Release);

        Ok(Some(RingMessageMeta { len, seq, flags }))
    }
}

impl Default for SpscRing {
    fn default() -> Self {
        Self::new()
    }
}

/// A bi-directional shared-memory channel connecting two cells (Endpoint A and Endpoint B).
pub struct BiRingChannel {
    /// Ring transmitting from A to B.
    pub a_to_b: SpscRing,
    /// Ring transmitting from B to A.
    pub b_to_a: SpscRing,
}

impl BiRingChannel {
    /// Construct a new bidirectional channel.
    pub const fn new() -> Self {
        Self {
            a_to_b: SpscRing::new(),
            b_to_a: SpscRing::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spsc_ring_empty_pop() {
        let ring = SpscRing::new();
        let mut buf = [0u8; 64];
        assert_eq!(ring.try_pop(&mut buf).unwrap(), None);
    }

    #[test]
    fn test_spsc_ring_push_pop_exact() {
        let ring = SpscRing::new();
        let msg = b"Hello Cellos Fastpath IPC!";
        ring.try_push(msg, 42, 7).unwrap();

        assert_eq!(ring.len(), 1);
        assert!(!ring.is_empty());
        assert!(!ring.is_full());

        let mut buf = [0u8; 64];
        let meta = ring.try_pop(&mut buf).unwrap().expect("should pop");
        assert_eq!(meta.len, msg.len());
        assert_eq!(meta.seq, 42);
        assert_eq!(meta.flags, 7);
        assert_eq!(&buf[..meta.len], msg);

        assert_eq!(ring.len(), 0);
        assert!(ring.is_empty());
    }

    #[test]
    fn test_spsc_ring_wraparound() {
        let ring = SpscRing::new();
        let mut buf = [0u8; 64];

        for i in 0..100 {
            let msg = [i as u8; 32];
            ring.try_push(&msg, i, 0).unwrap();
            let meta = ring.try_pop(&mut buf).unwrap().unwrap();
            assert_eq!(meta.seq, i);
            assert_eq!(&buf[..meta.len], &msg[..]);
        }
    }

    #[test]
    fn test_spsc_ring_full() {
        let ring = SpscRing::new();
        let msg = [0x55u8; 16];

        for i in 0..RING_CAPACITY {
            ring.try_push(&msg, i as u32, 0).unwrap();
        }
        assert!(ring.is_full());
        assert_eq!(ring.try_push(&msg, 999, 0), Err(RingError::Full));
    }

    #[test]
    fn test_spsc_ring_oversize() {
        let ring = SpscRing::new();
        let big_msg = [0xaa; 65];
        assert_eq!(
            ring.try_push(&big_msg, 0, 0),
            Err(RingError::MessageTooLarge)
        );
    }
}

impl Default for BiRingChannel {
    fn default() -> Self {
        Self::new()
    }
}
