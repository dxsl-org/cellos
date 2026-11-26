//! The bounded queue of client PCM the device has not been given yet.
//!
//! A fixed-capacity byte ring: a client write is accepted whole or not at all
//! ([`PcmQueue::spans`]), and the device side takes up to a period at a time
//! ([`PcmQueue::pop_into`]). Bytes leave in the order they arrived.

use alloc::vec;
use alloc::vec::Vec;

/// Queue capacity: 200 ms of 48 kHz S16 stereo.
pub const CAPACITY: usize = 38_400;

pub struct PcmQueue {
    buf: Vec<u8>,
    head: usize,
    len: usize,
}

impl PcmQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0; capacity],
            head: 0,
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn free(&self) -> usize {
        self.buf.len() - self.len
    }

    /// The free space for the next `n` bytes, in ring order (the second
    /// span is empty unless the space wraps); `None` when `n` does not fit.
    /// Nothing is queued until [`PcmQueue::commit`].
    pub fn spans(&mut self, n: usize) -> Option<(&mut [u8], &mut [u8])> {
        if n > self.free() {
            return None;
        }
        let cap = self.buf.len();
        let tail = (self.head + self.len) % cap.max(1);
        let first = n.min(cap - tail);
        let (front, back) = self.buf.split_at_mut(tail);
        Some((&mut back[..first], &mut front[..n - first]))
    }

    /// Queues the `n` bytes last written through [`PcmQueue::spans`].
    pub fn commit(&mut self, n: usize) {
        assert!(n <= self.free());
        self.len += n;
    }

    /// Moves the oldest `min(len, out.len())` bytes into `out`; returns how
    /// many.
    pub fn pop_into(&mut self, out: &mut [u8]) -> usize {
        let n = self.len.min(out.len());
        let cap = self.buf.len();
        let first = n.min(cap - self.head);
        out[..first].copy_from_slice(&self.buf[self.head..self.head + first]);
        out[first..n].copy_from_slice(&self.buf[..n - first]);
        self.head = (self.head + n) % cap.max(1);
        self.len -= n;
        n
    }

    /// Drops everything queued.
    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push(queue: &mut PcmQueue, bytes: &[u8]) -> bool {
        let Some((a, b)) = queue.spans(bytes.len()) else {
            return false;
        };
        let split = a.len();
        a.copy_from_slice(&bytes[..split]);
        b.copy_from_slice(&bytes[split..]);
        queue.commit(bytes.len());
        true
    }

    #[test]
    fn writes_are_accepted_whole_or_refused() {
        let mut queue = PcmQueue::new(8);
        assert!(push(&mut queue, &[1, 2, 3, 4, 5, 6]));
        assert!(!push(&mut queue, &[7, 8, 9]), "3 bytes do not fit in 2");
        assert_eq!(queue.len(), 6);
        assert!(push(&mut queue, &[7, 8]));
        assert_eq!(queue.free(), 0);
        assert!(!push(&mut queue, &[9]));
    }

    #[test]
    fn bytes_leave_in_order_across_the_wrap() {
        let mut queue = PcmQueue::new(8);
        assert!(push(&mut queue, &[1, 2, 3, 4, 5, 6]));
        let mut out = [0u8; 4];
        assert_eq!(queue.pop_into(&mut out), 4);
        assert_eq!(out, [1, 2, 3, 4]);
        assert!(push(&mut queue, &[7, 8, 9, 10, 11]), "wraps at 8");
        let mut out = [0u8; 16];
        assert_eq!(queue.pop_into(&mut out), 7);
        assert_eq!(&out[..7], &[5, 6, 7, 8, 9, 10, 11]);
        assert!(queue.is_empty());
        assert_eq!(queue.pop_into(&mut out), 0);
    }

    #[test]
    fn spans_split_exactly_at_the_end_of_the_buffer() {
        let mut queue = PcmQueue::new(CAPACITY);
        assert!(push(&mut queue, &vec![0; CAPACITY - 100]));
        let mut out = vec![0; CAPACITY - 200];
        queue.pop_into(&mut out);
        let (a, b) = queue.spans(300).unwrap();
        assert_eq!((a.len(), b.len()), (100, 200));
        queue.clear();
        assert_eq!(queue.free(), CAPACITY);
    }
}
