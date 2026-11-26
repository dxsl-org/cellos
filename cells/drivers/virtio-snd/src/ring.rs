//! Split-virtqueue geometry (virtio 1.2 §2.7) and the cell's DMA region map.
//! Pure: offsets and index math only, host-tested.
//!
//! One DMA allocation of [`DMA_PAGES`] pages holds everything the device
//! touches:
//!
//! | offset                     | contents                                    |
//! |----------------------------|---------------------------------------------|
//! | `q * RING_STRIDE`, q 0..4  | descriptor table, avail and used ring of q  |
//! | [`CTRL_REQ_OFF`]           | control request (device-readable)           |
//! | [`CTRL_RESP_OFF`]          | control response (device-writable)          |
//! | [`EVENT_OFF`]              | event buffers (device-writable)             |
//! | [`TX_OFF`] + `s * TX_SLOT` | tx slot s: xfer header, one period, status  |

use crate::proto::{EVENT_LEN, PERIODS, PERIOD_BYTES, XFER_LEN};

pub const PAGE: usize = 4096;
/// Bytes reserved for each queue's three rings.
pub const RING_STRIDE: usize = 512;
/// controlq, eventq, txq, rxq.
pub const QUEUES: u16 = 4;

pub const CTRL_REQ_OFF: usize = RING_STRIDE * QUEUES as usize;
pub const CTRL_REQ_LEN: usize = 64;
pub const CTRL_RESP_OFF: usize = CTRL_REQ_OFF + CTRL_REQ_LEN;
pub const CTRL_RESP_LEN: usize = 64;
pub const EVENT_OFF: usize = CTRL_RESP_OFF + CTRL_RESP_LEN;
/// Event buffers kept posted; events only need recycling, not reading.
pub const EVENT_BUFS: u16 = 4;

pub const TX_OFF: usize = PAGE;
pub const TX_SLOT: usize = 2048;
/// Offsets inside a tx slot: header and period form one device-readable
/// span, the status lands after them, 8-aligned.
pub const TX_PCM: usize = XFER_LEN;
pub const TX_STATUS: usize = (XFER_LEN + PERIOD_BYTES).next_multiple_of(8);
/// Pages in the DMA allocation.
pub const DMA_PAGES: usize = (TX_OFF + PERIODS * TX_SLOT).div_ceil(PAGE);

/// `VIRTQ_DESC_F_NEXT`.
pub const DESC_F_NEXT: u16 = 1;
/// `VIRTQ_DESC_F_WRITE`: device-writable buffer.
pub const DESC_F_WRITE: u16 = 2;
/// `VIRTQ_AVAIL_F_NO_INTERRUPT`: the driver polls.
pub const AVAIL_F_NO_INTERRUPT: u16 = 1;
const DESC_LEN: usize = 16;
const USED_ELEM_LEN: usize = 8;
/// Largest ring that fits [`RING_STRIDE`].
pub const MAX_QUEUE_SIZE: u16 = 16;

/// Queue size to program: the largest power of two not above the device
/// maximum or `wanted`; `None` when that holds fewer than `chain` descriptors.
pub fn queue_size(device_max: u32, wanted: u16, chain: u16) -> Option<u16> {
    let cap = device_max.min(u32::from(wanted.min(MAX_QUEUE_SIZE)));
    if cap == 0 {
        return None;
    }
    let size = 1u16 << (31 - cap.leading_zeros());
    (size >= chain).then_some(size)
}

/// Ring offsets of one queue inside the DMA allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingLayout {
    pub size: u16,
    /// Descriptor table (16-byte aligned).
    pub desc: usize,
    /// `struct virtq_avail` (2-byte aligned).
    pub avail: usize,
    /// `struct virtq_used` (4-byte aligned).
    pub used: usize,
    /// First byte past the used ring.
    pub end: usize,
}

impl RingLayout {
    /// The rings of queue `queue` with `size` entries.
    pub fn new(queue: u16, size: u16) -> Self {
        let n = usize::from(size);
        let desc = RING_STRIDE * usize::from(queue);
        let avail = desc + DESC_LEN * n;
        let used = (avail + 6 + 2 * n).next_multiple_of(4);
        Self {
            size,
            desc,
            avail,
            used,
            end: used + 6 + USED_ELEM_LEN * n,
        }
    }

    pub fn desc_entry(&self, index: u16) -> usize {
        self.desc + DESC_LEN * usize::from(index)
    }

    pub fn avail_idx(&self) -> usize {
        self.avail + 2
    }

    /// Avail ring entry receiving the chain published as `avail_idx`.
    pub fn avail_entry(&self, avail_idx: u16) -> usize {
        self.avail + 4 + 2 * usize::from(avail_idx % self.size)
    }

    pub fn used_idx(&self) -> usize {
        self.used + 2
    }

    /// Used ring element for the completion counted as `used_idx`.
    pub fn used_entry(&self, used_idx: u16) -> usize {
        self.used + 4 + USED_ELEM_LEN * usize::from(used_idx % self.size)
    }
}

/// Start of tx slot `slot`.
pub fn tx_slot(slot: u16) -> usize {
    TX_OFF + TX_SLOT * usize::from(slot)
}

/// Start of event buffer `slot`.
pub fn event_buf(slot: u16) -> usize {
    EVENT_OFF + EVENT_LEN * usize::from(slot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_size_is_a_bounded_power_of_two() {
        assert_eq!(queue_size(0, 16, 2), None);
        assert_eq!(queue_size(1, 16, 2), None);
        assert_eq!(queue_size(2, 16, 2), Some(2));
        assert_eq!(queue_size(12, 16, 2), Some(8));
        assert_eq!(queue_size(256, 16, 2), Some(16));
        assert_eq!(queue_size(256, 4, 1), Some(4));
        assert_eq!(queue_size(u32::MAX, u16::MAX, 2), Some(MAX_QUEUE_SIZE));
    }

    #[test]
    fn rings_are_aligned_and_stay_in_their_stride() {
        for queue in 0..QUEUES {
            let ring = RingLayout::new(queue, MAX_QUEUE_SIZE);
            assert_eq!(ring.desc % 16, 0);
            assert_eq!(ring.avail % 2, 0);
            assert_eq!(ring.used % 4, 0);
            assert!(ring.end <= ring.desc + RING_STRIDE);
        }
        assert_eq!(
            RingLayout::new(2, 16),
            RingLayout {
                size: 16,
                desc: 1024,
                avail: 1280,
                used: 1320,
                end: 1454
            }
        );
    }

    #[test]
    fn regions_do_not_overlap_and_fit_the_allocation() {
        assert!(event_buf(EVENT_BUFS) <= TX_OFF);
        assert!(TX_STATUS + crate::proto::PCM_STATUS_LEN <= TX_SLOT);
        assert_eq!(TX_STATUS % 8, 0);
        assert!(tx_slot(PERIODS as u16) <= DMA_PAGES * PAGE);
        assert!(CTRL_RESP_LEN >= crate::proto::STATUS_LEN + crate::proto::PCM_INFO_LEN);
    }

    #[test]
    fn ring_indices_wrap() {
        let ring = RingLayout::new(0, 4);
        assert_eq!(ring.avail_entry(5), ring.avail + 4 + 2);
        assert_eq!(ring.used_entry(u16::MAX), ring.used + 4 + 8 * 3);
    }
}
