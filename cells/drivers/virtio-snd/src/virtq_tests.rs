//! Split-queue tests against a fake device reading and writing the same
//! byte-addressed memory.

use crate::ring::{DESC_F_NEXT, DESC_F_WRITE};
use crate::virtq::{BadCompletion, Mem, SplitQueue};
use std::cell::RefCell;

const IOVA: u64 = 0x8000_0000;

struct FakeMem(RefCell<Vec<u8>>);

impl FakeMem {
    fn new() -> Self {
        Self(RefCell::new(vec![0xAA; 8192]))
    }

    fn u16(&self, off: usize) -> u16 {
        let mem = self.0.borrow();
        u16::from_le_bytes([mem[off], mem[off + 1]])
    }
}

impl Mem for FakeMem {
    fn write(&self, off: usize, bytes: &[u8]) {
        self.0.borrow_mut()[off..off + bytes.len()].copy_from_slice(bytes);
    }

    fn read(&self, off: usize, out: &mut [u8]) {
        out.copy_from_slice(&self.0.borrow()[off..off + out.len()]);
    }

    fn iova(&self, off: usize) -> u64 {
        IOVA + off as u64
    }
}

/// The device half: what it consumed from the avail ring and posted used.
struct FakeDevice {
    seen_avail: u16,
    used: u16,
}

impl FakeDevice {
    /// Takes the next available chain: its head and descriptors as
    /// (offset, len, flags, next).
    fn take(
        &mut self,
        mem: &FakeMem,
        q: &SplitQueue,
    ) -> Option<(u16, Vec<(usize, u32, u16, u16)>)> {
        if mem.u16(q.ring.avail_idx()) == self.seen_avail {
            return None;
        }
        let head = mem.u16(q.ring.avail_entry(self.seen_avail));
        self.seen_avail = self.seen_avail.wrapping_add(1);
        let mut chain = Vec::new();
        let mut index = head;
        loop {
            let mut desc = [0u8; 16];
            mem.read(q.ring.desc_entry(index), &mut desc);
            let addr = u64::from_le_bytes(desc[0..8].try_into().unwrap());
            let len = u32::from_le_bytes(desc[8..12].try_into().unwrap());
            let flags = u16::from_le_bytes([desc[12], desc[13]]);
            let next = u16::from_le_bytes([desc[14], desc[15]]);
            chain.push(((addr - IOVA) as usize, len, flags, next));
            if flags & DESC_F_NEXT == 0 {
                break;
            }
            index = next;
        }
        Some((head, chain))
    }

    fn complete(&mut self, mem: &FakeMem, q: &SplitQueue, id: u32, len: u32) {
        let mut elem = [0u8; 8];
        elem[0..4].copy_from_slice(&id.to_le_bytes());
        elem[4..8].copy_from_slice(&len.to_le_bytes());
        mem.write(q.ring.used_entry(self.used), &elem);
        self.used = self.used.wrapping_add(1);
        mem.write(q.ring.used_idx(), &self.used.to_le_bytes());
    }
}

fn setup(size: u16, chain: u16) -> (FakeMem, SplitQueue, FakeDevice) {
    let mem = FakeMem::new();
    let mut q = SplitQueue::new(2, size, chain);
    q.reset(&mem);
    let device = FakeDevice {
        seen_avail: 0,
        used: 0,
    };
    (mem, q, device)
}

#[test]
fn reset_zeroes_the_rings_and_disables_interrupts() {
    let (mem, q, _) = setup(16, 2);
    assert_eq!(mem.u16(q.ring.avail), 1);
    assert_eq!(mem.u16(q.ring.avail_idx()), 0);
    assert_eq!(mem.u16(q.ring.used_idx()), 0);
    assert_eq!(q.slots(), 8);
    assert_eq!(q.areas(&mem), (IOVA + 1024, IOVA + 1280, IOVA + 1320));
}

#[test]
fn a_chain_reaches_the_device_and_its_slot_comes_back() {
    let (mem, mut q, mut device) = setup(16, 2);
    let slot = q.free_slot().unwrap();
    assert_eq!(slot, 0);
    q.submit(&mem, slot, &[(4096, 1924, false), (6024, 8, true)]);
    assert_eq!(q.in_flight(), 1);
    assert_eq!(q.free_slot(), Some(1));
    let (head, chain) = device.take(&mem, &q).unwrap();
    assert_eq!(head, 0);
    assert_eq!(
        chain,
        vec![(4096, 1924, DESC_F_NEXT, 1), (6024, 8, DESC_F_WRITE, 0)]
    );
    assert_eq!(q.pop_used(&mem), Ok(None));
    device.complete(&mem, &q, u32::from(head), 8);
    assert_eq!(q.pop_used(&mem), Ok(Some((0, 8))));
    assert_eq!(q.in_flight(), 0);
    assert_eq!(q.pop_used(&mem), Ok(None));
}

#[test]
fn out_of_order_completions_free_the_right_slots() {
    let (mem, mut q, mut device) = setup(8, 2);
    for slot in 0..4 {
        q.submit(&mem, slot, &[(4096, 4, false), (5000, 8, true)]);
    }
    assert_eq!(q.free_slot(), None);
    let heads: Vec<u16> = (0..4).map(|_| device.take(&mem, &q).unwrap().0).collect();
    assert_eq!(heads, vec![0, 2, 4, 6]);
    device.complete(&mem, &q, 4, 0);
    device.complete(&mem, &q, 0, 0);
    assert_eq!(q.pop_used(&mem), Ok(Some((2, 0))));
    assert_eq!(q.pop_used(&mem), Ok(Some((0, 0))));
    assert_eq!(q.free_slot(), Some(0));
    assert_eq!(q.in_flight(), 2);
}

#[test]
fn indices_wrap_through_u16() {
    let (mem, mut q, mut device) = setup(4, 1);
    for round in 0..70_000u32 {
        q.submit(&mem, 0, &[(4096, 8, true)]);
        let (head, _) = device.take(&mem, &q).unwrap();
        device.complete(&mem, &q, u32::from(head), round);
        assert_eq!(q.pop_used(&mem), Ok(Some((0, round))));
    }
    assert_eq!(mem.u16(q.ring.avail_idx()), (70_000u32 % 65_536) as u16);
}

#[test]
fn bogus_completions_are_refused() {
    let (mem, mut q, mut device) = setup(8, 2);
    q.submit(&mem, 0, &[(4096, 4, false), (5000, 8, true)]);
    // A non-head descriptor id.
    device.complete(&mem, &q, 1, 0);
    assert_eq!(q.pop_used(&mem), Err(BadCompletion));

    let (mem, mut q, mut device) = setup(8, 2);
    q.submit(&mem, 0, &[(4096, 4, false), (5000, 8, true)]);
    // A slot that is not in flight.
    device.complete(&mem, &q, 2, 0);
    assert_eq!(q.pop_used(&mem), Err(BadCompletion));

    let (mem, mut q, mut device) = setup(8, 2);
    // More completions than buffers in flight.
    device.complete(&mem, &q, 0, 0);
    assert_eq!(q.pop_used(&mem), Err(BadCompletion));
}
