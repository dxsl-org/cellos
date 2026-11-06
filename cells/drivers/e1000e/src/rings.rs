//! Legacy TX/RX descriptor rings in `GrantDma`-authorized memory.
//!
//! The 82574 keeps the 8254x **legacy** descriptor formats (datasheet §7.1.4
//! receive, §7.2.10 transmit), selected by `RFCTL.EXSTEN = 0` and
//! `RCTL.DTYP = 00` (both reset defaults). Each descriptor is 16 bytes; one
//! 4 KiB ring page holds every slot this cell uses.
//!
//! Law 4 exception: the volatile accesses below touch device-visible DMA memory.
//! Every ring and buffer is a whole `DmaBuf` page that the cell owns for its
//! lifetime, and every index is bounded by the slot count before the access.

use core::mem::size_of;
use core::sync::atomic::{fence, Ordering};
use ostd::dma::DmaBuf;

/// Legacy transmit descriptor.
#[repr(C)]
pub(crate) struct TxDesc {
    pub buf_addr: u64,
    pub length: u16,
    pub cso: u8,
    pub cmd: u8,
    pub status: u8,
    pub css: u8,
    pub special: u16,
}

/// Legacy receive descriptor; the controller writes everything after `buf_addr`.
#[repr(C)]
pub(crate) struct RxDesc {
    pub buf_addr: u64,
    pub length: u16,
    pub checksum: u16,
    pub status: u8,
    pub errors: u8,
    pub special: u16,
}

const _: () = assert!(size_of::<TxDesc>() == 16);
const _: () = assert!(size_of::<RxDesc>() == 16);

pub(crate) const TXD_CMD_EOP: u8 = 1 << 0;
pub(crate) const TXD_CMD_IFCS: u8 = 1 << 1;
pub(crate) const TXD_CMD_RS: u8 = 1 << 3;
/// Command for a single-buffer frame: end of packet, insert FCS, report status.
pub(crate) const TXD_CMD_FRAME: u8 = TXD_CMD_EOP | TXD_CMD_IFCS | TXD_CMD_RS;
pub(crate) const DESC_STATUS_DD: u8 = 1 << 0;
/// RX status: last descriptor of a frame.
pub(crate) const RXD_STAT_EOP: u8 = 1 << 1;

/// Largest frame moved in one descriptor; `RCTL.BSIZE = 00` is 2048 bytes.
pub const BUF_SIZE: usize = 2048;
const _: () = assert!(BUF_SIZE <= 4096, "one frame fits one DMA page");

/// Ring length in bytes for `slots` legacy descriptors (`TDLEN`/`RDLEN`).
pub(crate) const fn ring_bytes(slots: usize) -> u32 {
    (slots * size_of::<TxDesc>()) as u32
}

/// Zero a ring page before its base is programmed.
pub(crate) fn clear(ring: &DmaBuf) {
    // SAFETY: `ring` is one owned, writable DMA page of `ring.size()` bytes.
    unsafe { core::ptr::write_bytes(ring.virt(), 0, ring.size()) };
}

/// Hand RX slot `slot` (< ring capacity) to the controller with buffer `iova`.
pub(crate) fn post_rx(ring: &DmaBuf, slot: usize, iova: u64) {
    assert!((slot + 1) * size_of::<RxDesc>() <= ring.size());
    // SAFETY: the slot lies inside the owned ring page (asserted above).
    unsafe {
        let desc = (ring.virt() as *mut RxDesc).add(slot);
        core::ptr::write_volatile(&mut (*desc).buf_addr, iova);
        core::ptr::write_volatile(&mut (*desc).length, 0);
        core::ptr::write_volatile(&mut (*desc).checksum, 0);
        core::ptr::write_volatile(&mut (*desc).status, 0);
        core::ptr::write_volatile(&mut (*desc).errors, 0);
        core::ptr::write_volatile(&mut (*desc).special, 0);
    }
}

/// A written-back RX descriptor, as [`rx_completed`] classifies it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RxCompletion {
    /// DD clear: the controller still owns the slot.
    Pending,
    /// A whole, error-free frame of this many bytes.
    Frame(usize),
    /// Hand the slot back without delivering it: an error was reported, or it
    /// is part of a frame spread over several descriptors.
    Drop,
}

/// RX slot `slot`'s completion. `in_fragment` is true while the descriptors
/// of a multi-descriptor frame are being dropped; it is updated here.
pub(crate) fn rx_completed(ring: &DmaBuf, slot: usize, in_fragment: &mut bool) -> RxCompletion {
    assert!((slot + 1) * size_of::<RxDesc>() <= ring.size());
    // SAFETY: the slot lies inside the owned ring page (asserted above).
    let desc = unsafe { (ring.virt() as *const RxDesc).add(slot) };
    // SAFETY: as above.
    let status = unsafe { core::ptr::read_volatile(&(*desc).status) };
    if status & DESC_STATUS_DD == 0 {
        return RxCompletion::Pending;
    }
    // The rest of the write-back and the frame bytes are valid only once DD
    // is seen: no read of them may move above the DD read.
    fence(Ordering::Acquire);
    // SAFETY: as above.
    let (errors, length) = unsafe {
        (
            core::ptr::read_volatile(&(*desc).errors),
            core::ptr::read_volatile(&(*desc).length),
        )
    };
    rx_written_back(status, errors, length, in_fragment)
}

/// Classify a descriptor whose DD bit is set. A frame is delivered only when
/// it fits one descriptor (EOP set, not the tail of a dropped frame) and the
/// controller reported no error.
pub(crate) fn rx_written_back(
    status: u8,
    errors: u8,
    length: u16,
    in_fragment: &mut bool,
) -> RxCompletion {
    let eop = status & RXD_STAT_EOP != 0;
    let tail_of_dropped = *in_fragment;
    *in_fragment = !eop;
    if !eop || tail_of_dropped || errors != 0 {
        RxCompletion::Drop
    } else {
        RxCompletion::Frame(length as usize)
    }
}

/// Fill TX slot `slot` for one frame of `len` bytes at `iova`.
pub(crate) fn post_tx(ring: &DmaBuf, slot: usize, iova: u64, len: u16) {
    assert!((slot + 1) * size_of::<TxDesc>() <= ring.size());
    // SAFETY: the slot lies inside the owned ring page (asserted above).
    unsafe {
        let desc = (ring.virt() as *mut TxDesc).add(slot);
        core::ptr::write_volatile(&mut (*desc).buf_addr, iova);
        core::ptr::write_volatile(&mut (*desc).length, len);
        core::ptr::write_volatile(&mut (*desc).cso, 0);
        core::ptr::write_volatile(&mut (*desc).cmd, TXD_CMD_FRAME);
        core::ptr::write_volatile(&mut (*desc).status, 0);
        core::ptr::write_volatile(&mut (*desc).css, 0);
        core::ptr::write_volatile(&mut (*desc).special, 0);
    }
}

/// Whether the controller wrote back DD for TX slot `slot`.
pub(crate) fn tx_done(ring: &DmaBuf, slot: usize) -> bool {
    tx_status(ring, slot).1 & DESC_STATUS_DD != 0
}

/// Whether TX slot `slot` may be (re)filled: never posted since the ring was
/// cleared, or written back (DD). A posted slot without DD still belongs to
/// the controller.
pub(crate) fn tx_slot_free(ring: &DmaBuf, slot: usize) -> bool {
    let (cmd, status) = tx_status(ring, slot);
    tx_slot_reusable(cmd, status)
}

fn tx_slot_reusable(cmd: u8, status: u8) -> bool {
    cmd == 0 || status & DESC_STATUS_DD != 0
}

/// `(cmd, status)` of TX slot `slot`.
fn tx_status(ring: &DmaBuf, slot: usize) -> (u8, u8) {
    assert!((slot + 1) * size_of::<TxDesc>() <= ring.size());
    // SAFETY: the slot lies inside the owned ring page (asserted above).
    unsafe {
        let desc = (ring.virt() as *const TxDesc).add(slot);
        (
            core::ptr::read_volatile(&(*desc).cmd),
            core::ptr::read_volatile(&(*desc).status),
        )
    }
}

/// Copy `frame` (≤ the buffer page) into the TX buffer `buf`.
pub(crate) fn copy_in(buf: &DmaBuf, frame: &[u8]) {
    assert!(frame.len() <= buf.size());
    // SAFETY: `buf` is an owned DMA page at least `frame.len()` bytes long.
    unsafe { core::ptr::copy_nonoverlapping(frame.as_ptr(), buf.virt(), frame.len()) };
}

/// Copy `len` received bytes (≤ the buffer page) from `buf` into `out`.
pub(crate) fn copy_out(buf: &DmaBuf, out: &mut [u8], len: usize) {
    assert!(len <= buf.size() && len <= out.len());
    // SAFETY: both ranges are `len` bytes inside their owners (asserted above).
    unsafe { core::ptr::copy_nonoverlapping(buf.virt(), out.as_mut_ptr(), len) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::offset_of;

    #[test]
    fn legacy_tx_descriptor_layout() {
        assert_eq!(offset_of!(TxDesc, buf_addr), 0);
        assert_eq!(offset_of!(TxDesc, length), 8);
        assert_eq!(offset_of!(TxDesc, cso), 10);
        assert_eq!(offset_of!(TxDesc, cmd), 11);
        assert_eq!(offset_of!(TxDesc, status), 12);
        assert_eq!(offset_of!(TxDesc, css), 13);
        assert_eq!(offset_of!(TxDesc, special), 14);
        assert_eq!(TXD_CMD_FRAME, 0x0B);
    }

    #[test]
    fn legacy_rx_descriptor_layout() {
        assert_eq!(offset_of!(RxDesc, buf_addr), 0);
        assert_eq!(offset_of!(RxDesc, length), 8);
        assert_eq!(offset_of!(RxDesc, checksum), 10);
        assert_eq!(offset_of!(RxDesc, status), 12);
        assert_eq!(offset_of!(RxDesc, errors), 13);
        assert_eq!(offset_of!(RxDesc, special), 14);
        assert_eq!(DESC_STATUS_DD, 0x01);
    }

    #[test]
    fn ring_fits_one_page_and_lengths_are_128_byte_multiples() {
        let bytes = ring_bytes(crate::dma_layout::TX_SLOTS);
        assert!(bytes as usize <= 4096);
        // TDLEN/RDLEN must be 128-byte aligned (datasheet §10.2.6/§10.2.7).
        assert_eq!(bytes % 128, 0);
        assert_eq!(ring_bytes(crate::dma_layout::RX_SLOTS) % 128, 0);
    }

    #[test]
    fn rx_delivers_only_whole_error_free_frames() {
        let mut in_fragment = false;
        let eop = DESC_STATUS_DD | RXD_STAT_EOP;
        assert_eq!(
            rx_written_back(eop, 0, 60, &mut in_fragment),
            RxCompletion::Frame(60)
        );
        // Any reported error (CRC, symbol, sequence, RX data) drops the frame.
        assert_eq!(
            rx_written_back(eop, 0x01, 60, &mut in_fragment),
            RxCompletion::Drop
        );
        assert_eq!(
            rx_written_back(eop, 0x80, 60, &mut in_fragment),
            RxCompletion::Drop
        );
        assert!(!in_fragment);
        // A frame over several descriptors is dropped up to and including EOP,
        // so its tail is never delivered as a frame of its own.
        assert_eq!(
            rx_written_back(DESC_STATUS_DD, 0, 2048, &mut in_fragment),
            RxCompletion::Drop
        );
        assert!(in_fragment);
        assert_eq!(
            rx_written_back(DESC_STATUS_DD, 0, 2048, &mut in_fragment),
            RxCompletion::Drop
        );
        assert_eq!(
            rx_written_back(eop, 0, 100, &mut in_fragment),
            RxCompletion::Drop
        );
        assert!(!in_fragment);
        assert_eq!(
            rx_written_back(eop, 0, 64, &mut in_fragment),
            RxCompletion::Frame(64)
        );
    }

    #[test]
    fn tx_slot_is_reusable_only_when_never_posted_or_written_back() {
        assert!(tx_slot_reusable(0, 0), "cleared ring");
        assert!(
            tx_slot_reusable(TXD_CMD_FRAME, DESC_STATUS_DD),
            "written back"
        );
        assert!(
            !tx_slot_reusable(TXD_CMD_FRAME, 0),
            "posted, still owned by the controller"
        );
    }
}
