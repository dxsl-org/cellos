//! Host tests for `rings.rs`: generation-bit wrap, descriptor encoding and
//! completion decoding against a byte-vector "DMA area".

use super::*;
use std::cell::RefCell;

/// A byte vector with the order of its 32/64-bit writes.
struct Area {
    bytes: Vec<u8>,
    order: RefCell<Vec<usize>>,
}

impl Area {
    fn new() -> Self {
        Self {
            bytes: vec![0; DMA_PAGES * 4096],
            order: RefCell::new(Vec::new()),
        }
    }
    fn u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.bytes[offset..offset + 4].try_into().unwrap())
    }
    fn set32(&mut self, offset: usize, value: u32) {
        self.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}

impl Sink for Area {
    fn put8(&mut self, offset: usize, value: u8) {
        self.bytes[offset] = value;
    }
    fn put16(&mut self, offset: usize, value: u16) {
        self.bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put32(&mut self, offset: usize, value: u32) {
        self.order.borrow_mut().push(offset);
        self.set32(offset, value);
    }
    fn put64(&mut self, offset: usize, value: u64) {
        self.order.borrow_mut().push(offset);
        self.bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
}

#[test]
fn generation_flips_exactly_on_wrap() {
    let mut ring = GenRing::new(TX_SLOTS);
    assert_eq!(
        (ring.next, ring.gen),
        (0, true),
        "both sides start at generation 1"
    );
    for _ in 0..TX_SLOTS - 1 {
        ring.advance();
    }
    assert_eq!((ring.next, ring.gen), (TX_SLOTS - 1, true));
    ring.advance();
    assert_eq!((ring.next, ring.gen), (0, false));
    for _ in 0..TX_SLOTS {
        ring.advance();
    }
    assert_eq!((ring.next, ring.gen), (0, true));
}

#[test]
fn tx_descriptor_encoding_writes_generation_last() {
    let mut area = Area::new();
    post_tx(&mut area, 3, 0xDEAD_B000, 342, true);
    let desc = TX_RING_OFF + 3 * 16;
    assert_eq!(area.u32(desc), 0xDEAD_B000);
    assert_eq!(
        area.u32(desc + 8),
        342 | 1 << 14,
        "len:14, gen at bit 14, dtype 0"
    );
    assert_eq!(
        area.u32(desc + 12),
        1 << 12 | 1 << 13,
        "eop | completion request, no offload"
    );
    assert_eq!(*area.order.borrow(), [desc, desc + 12, desc + 8]);
    post_tx(&mut area, TX_SLOTS + 3, 0x1000, 60, false);
    assert_eq!(area.u32(desc + 8), 60, "slot wraps; gen 0 clears bit 14");
}

#[test]
fn rx_descriptor_encoding_is_a_head_buffer() {
    let mut area = Area::new();
    post_rx(&mut area, 1, 0x4_2000, true);
    let desc = RX_RING_OFF[0] + 16;
    assert_eq!(area.u32(desc), 0x4_2000);
    assert_eq!(
        area.u32(desc + 8),
        BUF_SIZE as u32 | 1 << 31,
        "len, btype head, gen bit 31"
    );
    assert_eq!(*area.order.borrow().last().unwrap(), desc + 8);
}

fn read(area: &Area) -> impl Fn(usize) -> u32 + '_ {
    move |offset| area.u32(offset)
}

#[test]
fn completions_are_taken_only_with_the_consumer_generation() {
    let mut area = Area::new();
    let mut ring = GenRing::new(TX_SLOTS);
    assert_eq!(
        tx_completion(read(&area), &ring),
        None,
        "zeroed ring is empty"
    );
    area.set32(TX_COMP_OFF, 0xFFFF_F000 | 7);
    area.set32(TX_COMP_OFF + 12, 1 << 31);
    assert_eq!(
        tx_completion(read(&area), &ring),
        Some(7),
        "index is 12 bits"
    );
    ring.next = TX_SLOTS - 1;
    ring.advance();
    assert_eq!(tx_completion(read(&area), &ring), None, "stale after wrap");

    let ring = GenRing::new(RX_COMP_SLOTS);
    area.set32(RX_COMP_OFF, 5 | 1 << 14 | 1 << 15);
    area.set32(RX_COMP_OFF + 8, 590);
    area.set32(RX_COMP_OFF + 12, 1 << 31);
    let rx = rx_completion(read(&area), &ring).unwrap();
    assert_eq!(
        rx,
        RxCompletion {
            index: 5,
            ring: 0,
            sop: true,
            eop: true,
            error: false,
            len: 590
        }
    );
    area.set32(RX_COMP_OFF, 2 | 1 << 16);
    area.set32(RX_COMP_OFF + 8, 1 << 14 | 0x3FFF);
    let rx = rx_completion(read(&area), &ring).unwrap();
    assert_eq!(
        (rx.ring, rx.sop, rx.error, rx.len),
        (1, false, true, 0x3FFF)
    );
}

#[test]
fn layout_fits_the_allocation_and_buffers_are_disjoint() {
    assert_eq!(DMA_PAGES, 35);
    assert_eq!(TX_BUF_OFF, 3 * 4096);
    const _: () = assert!(TX_BUF_OFF + TX_SLOTS * BUF_SIZE <= RX_BUF_OFF);
    assert_eq!(RX_RING_OFF[1] - RX_RING_OFF[0], RX_SLOTS * 16);
}
