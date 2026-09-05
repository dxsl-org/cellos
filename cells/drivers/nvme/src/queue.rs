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

pub struct Queue {
    pub sq_buf: AuthorizedDma<DmaBuf>,
    pub cq_buf: AuthorizedDma<DmaBuf>,
    pub depth: u16,
    pub sq_tail: u16,
    pub cq_head: u16,
    pub cq_phase: bool,
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
            cq_head: 0,
            cq_phase: true, // initial phase = 1 after reset
            cid: 0,
        })
    }

    /// Device-visible base address of the SQ.
    #[inline]
    pub fn sq_iova(&self) -> u64 {
        self.sq_buf.iova()
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

    /// Shared pointer to the CQ entry at `idx`.
    ///
    /// # Safety
    /// `idx < depth` must hold.
    #[inline]
    pub unsafe fn cq_entry(&self, idx: usize) -> *const CqEntry {
        // SAFETY: caller guarantees idx < depth; cq_buf covers depth*16 bytes.
        unsafe { (self.cq_buf.inner().virt() as *const CqEntry).add(idx) }
    }

    pub fn next_cid(&mut self) -> u16 {
        self.cid = self.cid.wrapping_add(1);
        self.cid
    }
}
