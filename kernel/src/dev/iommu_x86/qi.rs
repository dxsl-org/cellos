//! VT-d queued invalidation (Intel VT-d spec §6.5.2, §10.4.22–10.4.25).
//!
//! The interrupt-entry cache can only be invalidated through the queue, so
//! interrupt remapping needs it. Once `GCMD.QIE` is set, register-based
//! context/IOTLB invalidation is illegal; every invalidation then goes through
//! [`InvQueue::submit`]. Submission is synchronous: each batch ends in an
//! invalidation-wait descriptor whose status write is polled.

use super::gcmd::{disable, issue_gcmd, wait_status};
use super::invalidate::{Scope, POLL_MAX};
use alloc::alloc::{alloc_zeroed, Layout};
use core::sync::atomic::{fence, Ordering};

/// `ECAP.QI`: queued invalidation supported.
pub const ECAP_QI: u64 = 1 << 1;
pub const VTD_GCMD_QIE: u32 = 1 << 26;
pub const VTD_GSTS_QIES: u32 = 1 << 26;

const VTD_FSTS: usize = 0x34;
const FSTS_IQE: u32 = 1 << 4;
const VTD_IQH: usize = 0x80;
const VTD_IQT: usize = 0x88;
const VTD_IQA: usize = 0x90;

/// `IQA.QS` = 0: one 4 KiB page of 256 128-bit descriptors (`IQA.DW` = 0).
const QUEUE_ENTRIES: u32 = 256;
const QUEUE_BYTES: usize = QUEUE_ENTRIES as usize * 16;

/// One 128-bit invalidation descriptor `[low, high]`.
pub type Descriptor = [u64; 2];

const TYPE_CONTEXT: u64 = 0x1;
const TYPE_IOTLB: u64 = 0x2;
const TYPE_IEC: u64 = 0x4;
const TYPE_WAIT: u64 = 0x5;

/// Granularity field (bits 5:4) shared by context and IOTLB descriptors.
fn granularity(scope: Scope) -> u64 {
    match scope {
        Scope::Global => 1 << 4,
        Scope::Domain(did) => 2 << 4 | (did as u64) << 16,
    }
}

/// Context-cache invalidation (§6.5.2.1).
pub fn context_desc(scope: Scope) -> Descriptor {
    [TYPE_CONTEXT | granularity(scope), 0]
}

/// IOTLB invalidation (§6.5.2.2); `drain` sets DR/DW, which the unit
/// honours only when GCAP reports read/write draining.
pub fn iotlb_desc(scope: Scope, drain_read: bool, drain_write: bool) -> Descriptor {
    let dr = (drain_read as u64) << 7;
    let dw = (drain_write as u64) << 6;
    [TYPE_IOTLB | granularity(scope) | dr | dw, 0]
}

/// Interrupt-entry-cache invalidation (§6.5.2.7): global for `None`, else the
/// single entry `index` (G=1, IM=0).
pub fn iec_desc(index: Option<u16>) -> Descriptor {
    match index {
        None => [TYPE_IEC, 0],
        Some(index) => [TYPE_IEC | 1 << 4 | (index as u64) << 32, 0],
    }
}

/// Invalidation wait (§6.5.2.8): once every earlier descriptor completes,
/// write `data` to the dword at `status_phys` (SW=1).
pub fn wait_desc(status_phys: u64, data: u32) -> Descriptor {
    [TYPE_WAIT | 1 << 5 | (data as u64) << 32, status_phys & !0x3]
}

/// Free descriptor slots in a ring of `size` given hardware `head` and
/// software `tail`; one slot stays empty so a full ring differs from empty.
pub fn free_slots(head: u32, tail: u32, size: u32) -> u32 {
    (head + size - tail - 1) % size
}

fn reg64(base: usize, off: usize) -> *mut u64 {
    (base + off) as *mut u64
}

pub struct InvQueue {
    virt: usize,
    phys: u64,
    status_virt: usize,
    status_phys: u64,
    /// Next descriptor index software writes.
    tail: u32,
    /// Last status value handed out; a late write of an older one never
    /// matches a newer wait.
    seq: u32,
}

impl InvQueue {
    /// Allocate the queue and turn on queued invalidation at the unit at
    /// `base`. `None` (queue off, register-based invalidation still legal)
    /// when the unit does not latch `GSTS.QIES`; the pages are then leaked on
    /// purpose, since a late-latching unit could still fetch from them.
    pub fn enable(base: usize) -> Option<Self> {
        // Firmware may hand over an enabled queue; IQA may only change while
        // QIE is clear.
        if !disable(base, VTD_GCMD_QIE, "QIES") {
            return None;
        }
        let page = Layout::from_size_align(QUEUE_BYTES, 4096).ok()?;
        let line = Layout::from_size_align(64, 64).ok()?;
        // SAFETY: both layouts are non-zero sized.
        let (virt, status_virt) =
            unsafe { (alloc_zeroed(page) as usize, alloc_zeroed(line) as usize) };
        if virt == 0 || status_virt == 0 {
            panic!("[vtd] out of memory allocating the invalidation queue");
        }
        let queue = Self {
            virt,
            phys: super::super::iommu_pt::virt_to_phys(virt),
            status_virt,
            status_phys: super::super::iommu_pt::virt_to_phys(status_virt),
            tail: 0,
            seq: 0,
        };
        // SAFETY: the register page is identity-mapped; the queue is 4 KiB
        // aligned. IQT must be 0 before QIE (the head resets to 0).
        unsafe {
            core::ptr::write_volatile(reg64(base, VTD_IQT), 0);
            core::ptr::write_volatile(reg64(base, VTD_IQA), queue.phys & !0xFFF);
        }
        issue_gcmd(base, VTD_GCMD_QIE);
        wait_status(base, VTD_GSTS_QIES, true, "QIES").then_some(queue)
    }

    fn head(&self, base: usize) -> u32 {
        // SAFETY: the register page is identity-mapped.
        let raw = unsafe { core::ptr::read_volatile(reg64(base, VTD_IQH)) };
        ((raw >> 4) as u32) % QUEUE_ENTRIES
    }

    fn put(&mut self, desc: Descriptor) {
        let slot = (self.virt + self.tail as usize * 16) as *mut u64;
        // SAFETY: tail < QUEUE_ENTRIES, inside the queue page; hardware does
        // not fetch the slot until IQT moves past it.
        unsafe {
            core::ptr::write_volatile(slot, desc[0]);
            core::ptr::write_volatile(slot.add(1), desc[1]);
        }
        self.tail = (self.tail + 1) % QUEUE_ENTRIES;
    }

    fn status(&self) -> u32 {
        // SAFETY: the status dword is a live allocation owned by the queue.
        unsafe { core::ptr::read_volatile(self.status_virt as *const u32) }
    }

    /// Queue `descs` plus a wait descriptor and poll for its completion.
    /// `false` on a full queue, a queue error, or timeout.
    pub fn submit(&mut self, base: usize, descs: &[Descriptor]) -> bool {
        let needed = descs.len() as u32 + 1;
        if free_slots(self.head(base), self.tail, QUEUE_ENTRIES) < needed {
            log::warn!("[vtd] unit {:#x} invalidation queue full", base);
            return false;
        }
        self.seq = self.seq.wrapping_add(1).max(1);
        for &desc in descs {
            self.put(desc);
        }
        self.put(wait_desc(self.status_phys, self.seq));
        // The descriptors (and any table stores they cover) must be visible
        // before the tail write hands them to hardware.
        fence(Ordering::SeqCst);
        // SAFETY: the register page is identity-mapped.
        unsafe { core::ptr::write_volatile(reg64(base, VTD_IQT), (self.tail as u64) << 4) };
        for _ in 0..POLL_MAX {
            if self.status() == self.seq {
                return true;
            }
            if self.recover_queue_error(base) {
                return false;
            }
            core::hint::spin_loop();
        }
        log::warn!(
            "[vtd] unit {:#x} invalidation wait timed out (seq={})",
            base,
            self.seq
        );
        false
    }

    /// After `FSTS.IQE` the unit stops at the faulting descriptor (at IQH).
    /// Replace it with a status-less wait and clear IQE so the queue drains.
    fn recover_queue_error(&mut self, base: usize) -> bool {
        // SAFETY: the register page is identity-mapped; FSTS is RW1C.
        let fsts = unsafe { core::ptr::read_volatile((base + VTD_FSTS) as *const u32) };
        if fsts & FSTS_IQE == 0 {
            return false;
        }
        let slot = (self.virt + self.head(base) as usize * 16) as *mut u64;
        // SAFETY: head < QUEUE_ENTRIES; hardware is halted on this slot.
        unsafe {
            core::ptr::write_volatile(slot, TYPE_WAIT);
            core::ptr::write_volatile(slot.add(1), 0);
            fence(Ordering::SeqCst);
            core::ptr::write_volatile((base + VTD_FSTS) as *mut u32, FSTS_IQE);
        }
        log::warn!("[vtd] unit {:#x} invalidation queue error cleared", base);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_encodings_match_the_spec() {
        assert_eq!(context_desc(Scope::Global), [0x11, 0]);
        assert_eq!(context_desc(Scope::Domain(0x1234)), [0x1234_0021, 0]);
        assert_eq!(iotlb_desc(Scope::Global, true, true), [0xD2, 0]);
        assert_eq!(iotlb_desc(Scope::Domain(7), false, false), [0x0007_0022, 0]);
        assert_eq!(iec_desc(None), [0x4, 0]);
        assert_eq!(iec_desc(Some(0x8001)), [0x8001_0000_0014, 0]);
        assert_eq!(
            wait_desc(0x1234_5678, 0xCAFE),
            [0xCAFE_0000_0025, 0x1234_5678]
        );
    }

    #[test]
    fn ring_keeps_one_slot_free() {
        assert_eq!(free_slots(0, 0, 256), 255);
        assert_eq!(free_slots(10, 9, 256), 0);
        assert_eq!(free_slots(0, 255, 256), 0);
        assert_eq!(free_slots(5, 250, 256), 10);
    }
}
