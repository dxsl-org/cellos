//! Legacy TX/RX descriptor rings in `GrantDma`-authorized memory.
//!
//! The 82574 keeps the 8254x **legacy** descriptor formats (datasheet §7.1.4
//! receive, §7.2.10 transmit), selected by `RFCTL.EXSTEN = 0` and
//! `RCTL.DTYP = 00` (both reset defaults). Each descriptor is 16 bytes; one
//! 4 KiB ring page holds every slot this cell uses. Volatile access to the
//! rings goes through `nic_driver::ring::DescRing`, which bounds every slot.

use core::mem::size_of;
use core::sync::atomic::{fence, Ordering};
use nic_driver::ring::DescRing;

/// TX descriptors (and TX buffers) in use.
pub(crate) const TX_SLOTS: usize = 16;
/// RX descriptors (and RX buffers) in use.
pub(crate) const RX_SLOTS: usize = 16;

pub(crate) type TxRing = DescRing<TxDesc, TX_SLOTS>;
pub(crate) type RxRing = DescRing<RxDesc, RX_SLOTS>;

/// Legacy transmit descriptor.
#[repr(C)]
#[derive(Clone, Copy)]
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
#[derive(Clone, Copy)]
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

/// Hand RX slot `slot` to the controller with buffer `iova`.
pub(crate) fn post_rx(ring: &RxRing, slot: usize, iova: u64) {
    ring.write(
        slot,
        RxDesc {
            buf_addr: iova,
            length: 0,
            checksum: 0,
            status: 0,
            errors: 0,
            special: 0,
        },
    );
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
pub(crate) fn rx_completed(ring: &RxRing, slot: usize, in_fragment: &mut bool) -> RxCompletion {
    let status = ring.read(slot).status;
    if status & DESC_STATUS_DD == 0 {
        return RxCompletion::Pending;
    }
    // The rest of the write-back and the frame bytes are valid only once DD
    // is seen: no read of them may move above the DD read, so the descriptor
    // is read again after the fence.
    fence(Ordering::Acquire);
    let desc = ring.read(slot);
    rx_written_back(status, desc.errors, desc.length, in_fragment)
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
pub(crate) fn post_tx(ring: &TxRing, slot: usize, iova: u64, len: u16) {
    ring.write(
        slot,
        TxDesc {
            buf_addr: iova,
            length: len,
            cso: 0,
            cmd: TXD_CMD_FRAME,
            status: 0,
            css: 0,
            special: 0,
        },
    );
}

/// Whether the controller wrote back DD for TX slot `slot`.
pub(crate) fn tx_done(ring: &TxRing, slot: usize) -> bool {
    tx_status(ring, slot).1 & DESC_STATUS_DD != 0
}

/// Whether TX slot `slot` of a `slots`-slot ring may be filled now: it and the
/// slot after it are both free. One slot always stays empty, so the tail
/// written after this post (`TDT = slot + 1`) never lands on a descriptor the
/// controller still owns — `TDT == TDH` would read as an empty ring.
pub(crate) fn tx_has_room(ring: &TxRing, slot: usize, slots: usize) -> bool {
    tx_room(slot, slots, |s| tx_status(ring, s))
}

fn tx_room(slot: usize, slots: usize, status: impl Fn(usize) -> (u8, u8)) -> bool {
    [slot, (slot + 1) % slots].into_iter().all(|s| {
        let (cmd, st) = status(s);
        tx_slot_reusable(cmd, st)
    })
}

/// A slot is free when it was never posted since the ring was cleared, or the
/// controller wrote it back (DD). A posted slot without DD still belongs to
/// the controller.
fn tx_slot_reusable(cmd: u8, status: u8) -> bool {
    cmd == 0 || status & DESC_STATUS_DD != 0
}

/// `(cmd, status)` of TX slot `slot`.
fn tx_status(ring: &TxRing, slot: usize) -> (u8, u8) {
    let desc = ring.read(slot);
    (desc.cmd, desc.status)
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
        let bytes = ring_bytes(TX_SLOTS);
        assert!(bytes as usize <= 4096);
        // TDLEN/RDLEN must be 128-byte aligned (datasheet §10.2.6/§10.2.7).
        assert_eq!(bytes % 128, 0);
        assert_eq!(ring_bytes(RX_SLOTS) % 128, 0);
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

    /// With nothing written back, a ring of N slots takes N - 1 frames: the
    /// tail never wraps onto a descriptor the controller still owns. One
    /// write-back at the slot after the tail frees room again.
    #[test]
    fn a_full_tx_ring_keeps_one_slot_empty() {
        use core::cell::RefCell;
        const SLOTS: usize = TX_SLOTS;
        let ring = RefCell::new([(0u8, 0u8); SLOTS]);
        let room = |slot| tx_room(slot, SLOTS, |s| ring.borrow()[s]);
        let mut next = 0;
        let mut posted = 0;
        while room(next) {
            ring.borrow_mut()[next] = (TXD_CMD_FRAME, 0);
            next = (next + 1) % SLOTS;
            posted += 1;
            assert!(posted < SLOTS, "the ring filled every slot");
        }
        assert_eq!(posted, SLOTS - 1);
        assert_eq!(next, SLOTS - 1, "the empty slot is the tail");
        // The head (slot 0) completes: the tail may advance onto it.
        ring.borrow_mut()[0].1 = DESC_STATUS_DD;
        assert!(room(next));
        // A tail slot itself still owned by the controller is refused too.
        ring.borrow_mut()[next] = (TXD_CMD_FRAME, 0);
        assert!(!room(next));
    }
}
