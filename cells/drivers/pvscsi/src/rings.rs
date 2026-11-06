//! Ring state, request and completion descriptor layouts and the DMA area map
//! (pure; host-tested). The encoders and decoders are in `proto.rs`.
//!
//! The `#[repr(C)]` mirrors follow FreeBSD's `pvscsi.h` (`pvscsi_rings_state`,
//! `pvscsi_ring_req_desc`, `pvscsi_ring_cmp_desc`,
//! `pvscsi_cmd_desc_setup_rings`; BSD-2-Clause, see `NOTICE`). They only
//! derive field offsets: every field is naturally aligned in the device
//! layout, so `repr(C)` reproduces it, and the tests pin offsets and sizes.

use core::mem::{offset_of, size_of};

#[repr(C)]
#[allow(dead_code)] // layout mirrors: only `offset_of!`/`size_of` read them
struct RingsState {
    req_prod_idx: u32,
    req_cons_idx: u32,
    req_num_entries_log2: u32,
    cmp_prod_idx: u32,
    cmp_cons_idx: u32,
    cmp_num_entries_log2: u32,
    req_call_threshold: u32,
    pad: [u8; 100],
    msg_prod_idx: u32,
    msg_cons_idx: u32,
    msg_num_entries_log2: u32,
}

#[repr(C)]
#[allow(dead_code)]
pub(crate) struct ReqDesc {
    pub(crate) context: u64,
    pub(crate) data_addr: u64,
    pub(crate) data_len: u64,
    pub(crate) sense_addr: u64,
    pub(crate) sense_len: u32,
    pub(crate) flags: u32,
    pub(crate) cdb: [u8; 16],
    pub(crate) cdb_len: u8,
    pub(crate) lun: [u8; 8],
    pub(crate) tag: u8,
    pub(crate) bus: u8,
    pub(crate) target: u8,
    pub(crate) vcpu_hint: u16,
    unused: [u8; 58],
}

#[repr(C)]
#[allow(dead_code)]
pub(crate) struct CmpDesc {
    pub(crate) context: u64,
    pub(crate) data_len: u64,
    pub(crate) sense_len: u32,
    pub(crate) host_status: u16,
    pub(crate) scsi_status: u16,
    pad: [u32; 2],
}

const MAX_RING_PAGES: usize = 32;

#[repr(C)]
#[allow(dead_code)]
struct SetupRings {
    req_ring_num_pages: u32,
    cmp_ring_num_pages: u32,
    rings_state_ppn: u64,
    req_ring_ppns: [u64; MAX_RING_PAGES],
    cmp_ring_ppns: [u64; MAX_RING_PAGES],
}

const PAGE: usize = 4096;
pub(crate) const REQ_LEN: usize = size_of::<ReqDesc>();
pub(crate) const CMP_LEN: usize = size_of::<CmpDesc>();
/// One page per ring: 32 request and 128 completion entries.
pub(crate) const REQ_ENTRIES: u32 = (PAGE / REQ_LEN) as u32;
pub(crate) const CMP_ENTRIES: u32 = (PAGE / CMP_LEN) as u32;
/// `SETUP_RINGS` descriptor length in 32-bit words.
pub(crate) const SETUP_WORDS: usize = size_of::<SetupRings>() / 4;

// ── The single DMA area: one page each, then the data bounce buffer ─────────

pub(crate) const STATE_OFF: usize = 0;
pub(crate) const REQ_RING_OFF: usize = PAGE;
pub(crate) const CMP_RING_OFF: usize = 2 * PAGE;
pub(crate) const SENSE_OFF: usize = 3 * PAGE;
pub(crate) const SENSE_LEN: usize = 96;
pub(crate) const DATA_OFF: usize = 4 * PAGE;
pub(crate) const DATA_LEN: usize = PAGE;
pub(crate) const DMA_PAGES: usize = (DATA_OFF + DATA_LEN) / PAGE;

/// Ring-state fields the cell reads or writes.
pub(crate) const REQ_PROD: usize = STATE_OFF + offset_of!(RingsState, req_prod_idx);
pub(crate) const REQ_CONS: usize = STATE_OFF + offset_of!(RingsState, req_cons_idx);
pub(crate) const REQ_LOG2: usize = STATE_OFF + offset_of!(RingsState, req_num_entries_log2);
pub(crate) const CMP_PROD: usize = STATE_OFF + offset_of!(RingsState, cmp_prod_idx);
pub(crate) const CMP_CONS: usize = STATE_OFF + offset_of!(RingsState, cmp_cons_idx);
pub(crate) const CMP_LOG2: usize = STATE_OFF + offset_of!(RingsState, cmp_num_entries_log2);

/// The descriptor of `SETUP_RINGS`: one page each for the ring state, the
/// request ring and the completion ring, by page number.
pub(crate) fn setup_rings_words(state: u64, req: u64, cmp: u64) -> [u32; SETUP_WORDS] {
    let mut words = [0u32; SETUP_WORDS];
    let mut put64 = |offset: usize, value: u64| {
        words[offset / 4] = value as u32;
        words[offset / 4 + 1] = (value >> 32) as u32;
    };
    put64(offset_of!(SetupRings, rings_state_ppn), state >> 12);
    put64(offset_of!(SetupRings, req_ring_ppns), req >> 12);
    put64(offset_of!(SetupRings, cmp_ring_ppns), cmp >> 12);
    words[offset_of!(SetupRings, req_ring_num_pages) / 4] = 1;
    words[offset_of!(SetupRings, cmp_ring_num_pages) / 4] = 1;
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_are_the_device_interface() {
        assert_eq!(size_of::<RingsState>(), 140);
        assert_eq!((REQ_PROD, REQ_CONS, REQ_LOG2), (0, 4, 8));
        assert_eq!((CMP_PROD, CMP_CONS, CMP_LOG2), (12, 16, 20));
        assert_eq!(offset_of!(RingsState, msg_prod_idx), 128);
        assert_eq!(REQ_LEN, 128);
        assert_eq!(offset_of!(ReqDesc, sense_len), 32);
        assert_eq!(offset_of!(ReqDesc, flags), 36);
        assert_eq!(offset_of!(ReqDesc, cdb), 40);
        assert_eq!(offset_of!(ReqDesc, cdb_len), 56);
        assert_eq!(offset_of!(ReqDesc, lun), 57);
        assert_eq!(offset_of!(ReqDesc, tag), 65);
        assert_eq!(offset_of!(ReqDesc, target), 67);
        assert_eq!(offset_of!(ReqDesc, vcpu_hint), 68);
        assert_eq!(CMP_LEN, 32);
        assert_eq!(offset_of!(CmpDesc, sense_len), 16);
        assert_eq!(offset_of!(CmpDesc, host_status), 20);
        assert_eq!(offset_of!(CmpDesc, scsi_status), 22);
        assert_eq!(SETUP_WORDS, 132);
        assert_eq!((REQ_ENTRIES, CMP_ENTRIES), (32, 128));
        assert_eq!(DMA_PAGES, 5);
    }

    #[test]
    fn setup_rings_carries_page_numbers_of_one_page_rings() {
        let words = setup_rings_words(0x1_2345_6000, 0x7000, 0x8000);
        assert_eq!(words[..4], [1, 1, 0x12_3456, 0], "one page each, state PPN");
        assert_eq!(words[4..6], [7, 0], "first request-ring PPN");
        assert_eq!(words[68..70], [8, 0], "first completion-ring PPN");
        assert!(words[6..68].iter().chain(&words[70..]).all(|&w| w == 0));
    }
}
