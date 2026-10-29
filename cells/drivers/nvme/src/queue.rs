//! NVMe Submission/Completion queue pair backed by DMA-allocated memory.
//!
//! Each queue pair consists of one Submission Queue (SQ) and one Completion
//! Queue (CQ).  Both are physically contiguous — allocated via `DmaBuf`.
//!
//! This file is `unsafe`-annotated because the queue pointers are raw DMA
//! memory, accessed via volatile reads/writes.
//!
// SAFETY invariant: `sq` and `cq` point into DMA-allocated contiguous pages
// that are valid for the lifetime of the Queue.  All accesses are volatile.

extern crate alloc;

use crate::dma::AuthorizedDma;
use core::mem::size_of;
use ostd::dma::DmaBuf;
use types::{ViError, ViResult};
use crate::deadline::Spent;
use core::sync::atomic::{fence, Ordering};

/// NVMe Submission Queue Entry (64 bytes, NVMe 1.x §4.2).
#[repr(C, align(64))]
#[derive(Clone, Copy, Default)]
pub struct SqEntry {
    pub cdw0: u32,
    pub nsid: u32,
    pub cdw2: u32,
    pub cdw3: u32,
    pub mptr: u64,
    pub prp1: u64,
    pub prp2: u64,
    pub cdw10: u32,
    pub cdw11: u32,
    pub cdw12: u32,
    pub cdw13: u32,
    pub cdw14: u32,
    pub cdw15: u32,
}

/// NVMe Completion Queue Entry (16 bytes, NVMe 1.x §4.4).
#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
pub struct CqEntry {
    pub dw0: u32,
    pub dw1: u32,
    pub sq_hd: u16,
    pub sq_id: u16,
    pub cid: u16,
    pub phase_status: u16, // phase bit[0], status[15:1]
}

impl CqEntry {
    // reason: phase()/status_field() decode the CQE per NVMe 1.x §4.4 but the
    // current completion poll reads phase_status directly inline; kept as the
    // documented accessor pair for callers that decode a CqEntry outside that loop.
    #[inline]
    #[allow(dead_code)]
    pub fn phase(&self) -> bool {
        self.phase_status & 1 != 0
    }
    #[inline]
    #[allow(dead_code)]
    pub fn status_field(&self) -> u16 {
        self.phase_status >> 1
    }
}

/// The next CQ slot and its expected phase, retained across commands.
pub(crate) struct CompletionCursor {
    pub head: u16,
    pub phase: bool,
    depth: u16,
}

impl CompletionCursor {
    fn new(depth: u16) -> Self {
        Self { head: 0, phase: true, depth }
    }

    /// One command deadline covers empty polls and every consumed late completion.
    /// Read the current cursor again after each acknowledgement, including at wrap.
    fn wait(
        &mut self,
        expected_cid: u16,
        mut read: impl FnMut(usize, bool) -> Option<(u16, u16)>,
        mut acknowledge: impl FnMut(u16),
        mut poll_spent: impl FnMut() -> Option<Spent>,
    ) -> Result<u16, Spent> {
        loop {
            if let Some((cid, status)) = read(self.head as usize, self.phase) {
                self.head += 1;
                if self.head == self.depth {
                    self.head = 0;
                    self.phase = !self.phase;
                }
                acknowledge(self.head);
                if cid == expected_cid {
                    return Ok(status);
                }
            }
            // An unexpected CID is progress, not an excuse to bypass the bound.
            if let Some(spent) = poll_spent() {
                return Err(spent);
            }
        }
    }
}

pub struct Queue {
    pub sq_buf: AuthorizedDma<DmaBuf>,
    pub cq_buf: AuthorizedDma<DmaBuf>,
    pub depth: u16,
    pub sq_tail: u16,
    pub cq: CompletionCursor,
    pub cid: u16,
}

impl Queue {
    /// Allocate and authorize a queue pair, retaining both returned IOVAs.
    pub fn new(bdf: u32, depth: u16) -> ViResult<Self> {
        let sq_pages = (depth as usize * size_of::<SqEntry>()).div_ceil(4096);
        let cq_pages = (depth as usize * size_of::<CqEntry>()).div_ceil(4096);

        let sq_buf = DmaBuf::alloc(sq_pages).ok_or(ViError::OutOfMemory)?;
        let cq_buf = DmaBuf::alloc(cq_pages).ok_or(ViError::OutOfMemory)?;

        // A rejected mapping fails closed before any queue address is
        // programmed into the controller.
        let sq_buf = AuthorizedDma::authorize(sq_buf, |buf| buf.authorize(bdf))
            .map_err(|_| ViError::PermissionDenied)?;
        let cq_buf = AuthorizedDma::authorize(cq_buf, |buf| buf.authorize(bdf))
            .map_err(|_| ViError::PermissionDenied)?;

        // Zero queues (DmaBuf is not guaranteed zeroed).
        unsafe {
            core::ptr::write_bytes(sq_buf.inner().virt(), 0, sq_buf.inner().size());
            core::ptr::write_bytes(cq_buf.inner().virt(), 0, cq_buf.inner().size());
        }

        Ok(Self {
            sq_buf,
            cq_buf,
            depth,
            sq_tail: 0,
            cq: CompletionCursor::new(depth),
            cid: 0,
        })
    }

    /// CPU-visible base address of the SQ (what the driver writes entries to).
    #[inline]
    pub fn sq_virt(&self) -> u64 {
        self.sq_buf.inner().virt() as u64
    }

    /// Device-visible base address of the SQ.
    #[inline]
    pub fn sq_iova(&self) -> u64 {
        self.sq_buf.iova()
    }

    /// CPU-visible base address of the CQ (what the driver polls).
    #[inline]
    pub fn cq_virt(&self) -> u64 {
        self.cq_buf.inner().virt() as u64
    }

    /// Device-visible base address of the CQ.
    #[inline]
    pub fn cq_iova(&self) -> u64 {
        self.cq_buf.iova()
    }

    /// Mutable pointer to the SQ entry at `idx`.
    ///
    /// # Safety
    /// `idx < depth` must hold.
    #[inline]
    pub unsafe fn sq_entry(&mut self, idx: usize) -> *mut SqEntry {
        // SAFETY: caller guarantees idx < depth; sq_buf covers depth*64 bytes.
        unsafe { (self.sq_buf.inner().virt() as *mut SqEntry).add(idx) }
    }

    /// Wait on DMA completion memory using the same cursor/deadline flow host tests exercise.
    pub fn wait_completion(
        &mut self,
        expected_cid: u16,
        acknowledge: impl FnMut(u16),
        poll_spent: impl FnMut() -> Option<Spent>,
    ) -> Result<u16, Spent> {
        let entries = self.cq_virt() as *const CqEntry;
        self.cq.wait(expected_cid, |index, phase| {
            // SAFETY: the cursor is always within depth and Queue owns the CQ allocation.
            let entry = unsafe { entries.add(index) };
            let raw = unsafe { core::ptr::read_volatile(&(*entry).phase_status) };
            if (raw & 1 != 0) != phase {
                return None;
            }
            // The phase publication precedes the rest of the completion's DMA payload.
            fence(Ordering::Acquire);
            let cid = unsafe { core::ptr::read_volatile(&(*entry).cid) };
            Some((cid, raw >> 1))
        }, acknowledge, poll_spent)
    }

    pub fn next_cid(&mut self) -> u16 {
        self.cid = self.cid.wrapping_add(1);
        self.cid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deadline::{Deadline, FALLBACK_POLLS, IO_BUDGET_MS, POLL_YIELD_EVERY};
    use core::cell::Cell;

    #[test]
    fn late_completions_advance_to_matching_success_or_error() {
        for status in [0, 0x81] {
            let mut cursor = CompletionCursor::new(4);
            let mut reads = alloc::vec::Vec::new();
            let mut acks = alloc::vec::Vec::new();
            let mut polls = 0;
            let result = cursor.wait(3, |index, phase| {
                reads.push((index, phase));
                Some(((index + 1) as u16, if index == 2 { status } else { 0 }))
            }, |head| acks.push(head), || { polls += 1; None });
            assert_eq!(result, Ok(status));
            assert_eq!(reads, [(0, true), (1, true), (2, true)]);
            assert_eq!(acks, [1, 2, 3]);
            assert_eq!(polls, 2);
        }
    }

    #[test]
    fn late_completion_at_last_slot_toggles_phase_before_next_read() {
        let mut cursor = CompletionCursor::new(2);
        cursor.head = 1;
        let mut reads = alloc::vec::Vec::new();
        let mut acks = alloc::vec::Vec::new();
        let result = cursor.wait(9, |index, phase| {
            reads.push((index, phase));
            Some((if index == 0 { 9 } else { 8 }, 0))
        }, |head| acks.push(head), || None);
        assert_eq!(result, Ok(0));
        assert_eq!(reads, [(1, true), (0, false)]);
        assert_eq!(acks, [0, 1]);
        assert!(!cursor.phase);
    }

    #[test]
    fn empty_or_wrong_phase_after_late_cqe_still_obeys_deadline() {
        let mut cursor = CompletionCursor::new(4);
        let mut reads = alloc::vec::Vec::new();
        let mut acks = alloc::vec::Vec::new();
        let polls = Cell::new(0);
        let result = cursor.wait(7, |index, phase| {
            reads.push((index, phase));
            if index == 0 { Some((6, 0)) } else { None }
        }, |head| acks.push(head), || {
            polls.set(polls.get() + 1);
            (polls.get() == 3).then_some(Spent::Millis(IO_BUDGET_MS))
        });
        assert_eq!(result, Err(Spent::Millis(IO_BUDGET_MS)));
        assert_eq!(reads, [(0, true), (1, true), (1, true)]);
        assert_eq!(acks, [1]);
    }

    #[test]
    fn always_ready_late_stream_keeps_original_clock_deadline_and_yield_cadence() {
        let mut cursor = CompletionCursor::new(64);
        let mut wait = Deadline::start(IO_BUDGET_MS, Some(100));
        let reads = Cell::new(0u64);
        let mut acks = 0;
        let mut yields = 0;
        let result = cursor.wait(99, |index, phase| {
            assert_eq!(index, (reads.get() % 64) as usize);
            assert_eq!(phase, (reads.get() / 64) % 2 == 0);
            reads.set(reads.get() + 1);
            Some((98, 0))
        }, |_| acks += 1, || {
            if wait.tick() {
                yields += 1;
                wait.check(Some(100 + IO_BUDGET_MS))
            } else { None }
        });
        assert_eq!(result, Err(Spent::Millis(IO_BUDGET_MS)));
        assert_eq!(reads.get(), POLL_YIELD_EVERY);
        assert_eq!(acks, POLL_YIELD_EVERY);
        assert_eq!(yields, 1);
    }

    #[test]
    fn always_ready_late_stream_keeps_no_clock_poll_bound() {
        let mut cursor = CompletionCursor::new(64);
        let mut wait = Deadline::start(IO_BUDGET_MS, None);
        let mut polls = 0u64;
        let result = cursor.wait(2, |_, _| Some((1, 0)), |_| {}, || {
            polls += 1;
            if wait.tick() { wait.check(None) } else { None }
        });
        assert_eq!(result, Err(Spent::Polls(polls)));
        assert!(polls >= FALLBACK_POLLS && polls < FALLBACK_POLLS + POLL_YIELD_EVERY);
    }
}
