//! Split-virtqueue geometry (virtio 1.2 §2.7) and the cell's DMA region map.
//! Pure: offsets and index math only, host-tested.
//!
//! One DMA allocation of [`DMA_PAGES`] pages holds everything the device
//! touches:
//!
//! | offset           | contents                                     |
//! |------------------|----------------------------------------------|
//! | 0                | descriptor table, avail ring, used ring      |
//! | [`REQ_OFF`]      | `virtio_scsi_req_cmd` device-readable header |
//! | [`RESP_OFF`]     | `virtio_scsi_resp_cmd` device-writable part  |
//! | [`DATA_OFF`]     | data-in / data-out bounce buffer             |

use crate::proto::{REQ_FIXED, RESP_FIXED};

/// Pages in the queue's DMA allocation.
pub const DMA_PAGES: usize = 2;
/// Bytes reserved for the three rings.
const RING_AREA: usize = 2048;
pub const REQ_OFF: usize = RING_AREA;
pub const REQ_LEN: usize = 512;
pub const RESP_OFF: usize = REQ_OFF + REQ_LEN;
pub const RESP_LEN: usize = 512;
pub const DATA_OFF: usize = 4096;
pub const DATA_LEN: usize = 4096;

/// Largest `cdb_size` / `sense_size` the request and response areas hold.
pub const MAX_CDB: usize = REQ_LEN - REQ_FIXED;
pub const MAX_SENSE: usize = RESP_LEN - RESP_FIXED;

/// Descriptors one command chains: header, response, one data segment.
pub const CHAIN_MAX: usize = 3;
/// Commands run one at a time, so a short ring suffices.
const MAX_QUEUE_SIZE: u16 = 8;

/// `VIRTQ_DESC_F_NEXT`.
pub const DESC_F_NEXT: u16 = 1;
/// `VIRTQ_DESC_F_WRITE`: device-writable buffer.
pub const DESC_F_WRITE: u16 = 2;
/// `VIRTQ_AVAIL_F_NO_INTERRUPT`: the driver polls.
pub const AVAIL_F_NO_INTERRUPT: u16 = 1;
const DESC_LEN: usize = 16;
const USED_ELEM_LEN: usize = 8;

/// Queue size to program: the largest power of two not above the device
/// maximum or [`MAX_QUEUE_SIZE`]; `None` when that cannot hold one chain.
pub fn queue_size(device_max: u32) -> Option<u16> {
    let cap = device_max.min(u32::from(MAX_QUEUE_SIZE));
    if cap == 0 {
        return None;
    }
    let size = 1u16 << (31 - cap.leading_zeros());
    (usize::from(size) >= CHAIN_MAX).then_some(size)
}

/// Ring offsets inside the DMA allocation; the descriptor table starts at 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingLayout {
    pub size: u16,
    /// `struct virtq_avail` (2-byte aligned).
    pub avail: usize,
    /// `struct virtq_used` (4-byte aligned).
    pub used: usize,
    /// First byte past the used ring.
    pub end: usize,
}

impl RingLayout {
    pub fn new(size: u16) -> Self {
        let n = usize::from(size);
        let avail = DESC_LEN * n;
        let used = (avail + 6 + 2 * n).next_multiple_of(4);
        Self {
            size,
            avail,
            used,
            end: used + 6 + USED_ELEM_LEN * n,
        }
    }

    pub fn desc(&self, index: u16) -> usize {
        DESC_LEN * usize::from(index)
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

/// Completions the device posted since `last_used` (wrapping 16-bit
/// indices).
pub fn completions(used_idx: u16, last_used: u16) -> u16 {
    used_idx.wrapping_sub(last_used)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_size_is_bounded_power_of_two() {
        assert_eq!(queue_size(0), None);
        assert_eq!(queue_size(2), None);
        assert_eq!(queue_size(4), Some(4));
        assert_eq!(queue_size(7), Some(4));
        assert_eq!(queue_size(256), Some(MAX_QUEUE_SIZE));
        assert_eq!(queue_size(u32::MAX), Some(MAX_QUEUE_SIZE));
    }

    #[test]
    fn rings_are_aligned_and_fit_their_area() {
        for size in [4, MAX_QUEUE_SIZE] {
            let ring = RingLayout::new(size);
            assert_eq!(ring.avail % 2, 0);
            assert_eq!(ring.used % 4, 0);
            assert!(ring.avail >= ring.desc(size));
            assert!(ring.end <= RING_AREA);
        }
        assert_eq!(
            RingLayout::new(8),
            RingLayout {
                size: 8,
                avail: 128,
                used: 152,
                end: 222
            }
        );
    }

    #[test]
    fn regions_do_not_overlap_and_fit_the_allocation() {
        assert!(RESP_OFF + RESP_LEN <= DATA_OFF);
        assert!(DATA_OFF + DATA_LEN <= DMA_PAGES * 4096);
        assert!(MAX_CDB >= 16 && MAX_SENSE >= 96);
    }

    #[test]
    fn ring_indices_wrap() {
        let ring = RingLayout::new(4);
        assert_eq!(ring.avail_entry(5), ring.avail + 4 + 2);
        assert_eq!(ring.used_entry(u16::MAX), ring.used + 4 + 8 * 3);
        assert_eq!(completions(0, u16::MAX), 1);
        assert_eq!(completions(7, 7), 0);
    }
}
