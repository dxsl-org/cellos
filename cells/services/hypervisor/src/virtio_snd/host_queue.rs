//! The guest's PCM on its way to the audio service (`client_audio`, on the
//! BSP's thread): bounded, oldest first, whole frames only.
//!
//! Sound is never worth stalling the guest for, so a full queue drops what
//! does not fit and counts it; so does a queue the client marked as not
//! accepting (no audio service, or a refused stream), without the device
//! reading the guest's buffer at all.

extern crate alloc;

use alloc::collections::VecDeque;

/// About 1.4 s of audio.
pub const CAPACITY: usize = 256 * 1024;
const FRAME: usize = wire::audio::FRAME_BYTES as usize;

pub struct HostQueue {
    bytes: VecDeque<u8>,
    accepting: bool,
    dropped: u64,
}

impl HostQueue {
    pub const fn new() -> Self {
        Self {
            bytes: VecDeque::new(),
            accepting: true,
            dropped: 0,
        }
    }

    /// Bytes that may still be pushed, whole frames.
    pub fn room(&self) -> usize {
        if !self.accepting {
            return 0;
        }
        (CAPACITY - self.bytes.len()) / FRAME * FRAME
    }

    /// Queue what fits of `bytes` (whole frames); the rest is dropped.
    pub fn push(&mut self, bytes: &[u8]) {
        let fit = bytes.len().min(self.room()) / FRAME * FRAME;
        self.bytes.extend(&bytes[..fit]);
        self.count_dropped((bytes.len() - fit) as u64);
    }

    /// Count `len` bytes dropped without queueing them.
    pub fn count_dropped(&mut self, len: u64) {
        self.dropped = self.dropped.saturating_add(len);
    }

    /// Whether pushes are taken; turning it off drops what is queued.
    pub fn set_accepting(&mut self, accepting: bool) {
        if !accepting {
            self.clear();
        }
        self.accepting = accepting;
    }

    /// Copy the oldest whole frames into `dst` without taking them; how many
    /// bytes.
    pub fn peek_into(&self, dst: &mut [u8]) -> usize {
        let len = dst.len().min(self.bytes.len()) / FRAME * FRAME;
        let (front, back) = self.bytes.as_slices();
        let from_front = len.min(front.len());
        dst[..from_front].copy_from_slice(&front[..from_front]);
        dst[from_front..len].copy_from_slice(&back[..len - from_front]);
        len
    }

    /// Take the `len` oldest bytes (after a [`Self::peek_into`] that saw them).
    pub fn consume(&mut self, len: usize) {
        self.bytes.drain(..len.min(self.bytes.len()));
    }

    /// Drop everything queued (stream release, device reset).
    pub fn clear(&mut self) {
        self.count_dropped(self.bytes.len() as u64);
        self.bytes.clear();
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Bytes dropped so far.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_queue_drops_and_counts_the_excess() {
        let mut queue = HostQueue::new();
        queue.push(&alloc::vec![1; CAPACITY - 8]);
        queue.push(&[2; 20]);
        assert_eq!(queue.len(), CAPACITY);
        assert_eq!(queue.dropped(), 12);
        assert_eq!(queue.room(), 0);
        queue.push(&[3; 4]);
        assert_eq!(queue.dropped(), 16);
    }

    #[test]
    fn only_whole_frames_move() {
        let mut queue = HostQueue::new();
        queue.push(&[1, 2, 3, 4, 5, 6]);
        assert_eq!((queue.len(), queue.dropped()), (4, 2));
        let mut dst = [0u8; 7];
        assert_eq!(queue.peek_into(&mut dst), 4);
        assert_eq!(&dst[..4], &[1, 2, 3, 4]);
        queue.consume(4);
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn peeking_spans_the_ring_and_consuming_takes_the_oldest() {
        let mut queue = HostQueue::new();
        queue.push(&[1; 8]);
        queue.consume(4);
        queue.push(&[2; 8]);
        let mut dst = [0u8; 12];
        assert_eq!(queue.peek_into(&mut dst), 12);
        assert_eq!(dst, [1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2]);
        queue.consume(8);
        assert_eq!(queue.peek_into(&mut dst), 4);
        assert_eq!(&dst[..4], &[2; 4]);
    }

    #[test]
    fn a_queue_not_accepting_drops_everything() {
        let mut queue = HostQueue::new();
        queue.push(&[1; 16]);
        queue.set_accepting(false);
        assert_eq!((queue.len(), queue.dropped()), (0, 16));
        queue.push(&[1; 8]);
        assert_eq!(queue.dropped(), 24);
        queue.set_accepting(true);
        queue.push(&[1; 8]);
        queue.clear();
        assert_eq!((queue.len(), queue.dropped()), (0, 32));
    }
}
