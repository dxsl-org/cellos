//! Device-side split-virtqueue steps for a queue whose buffers complete after
//! the notify that brought them (the txq completes at playback pace): take
//! one available chain, publish one used element. Every guest index is
//! checked as in `virtqueue::process_notify`.

extern crate alloc;

use alloc::vec::Vec;

use crate::virtio_mmio::QueueCfg;
use crate::virtqueue::{read_descriptor_chain, DescBuf};
use crate::virtqueue_guard::{avail_entry_gpa, checked_gpa, pending_count, used_entry_gpa};

/// The device's counters for one queue.
#[derive(Default)]
pub struct Ring {
    last_avail: u16,
    used: u16,
}

impl Ring {
    /// The next available chain's head and its descriptors (`None` for a chain
    /// that failed validation, which is completed with nothing written). `None`
    /// when the ring is empty, unreadable or its index is corrupt.
    pub fn pop(&mut self, vm_id: usize, qcfg: &QueueCfg) -> Option<(u16, Option<Vec<DescBuf>>)> {
        if !qcfg.ready || !qcfg.is_valid() {
            return None;
        }
        let q_size = usize::from(qcfg.num);
        let mut b2 = [0u8; 2];
        let avail_idx_gpa = checked_gpa(qcfg.avail_gpa, 2, 2)?;
        if crate::vmm::read_guest_memory(vm_id, avail_idx_gpa, &mut b2) != 2 {
            return None;
        }
        let avail_idx = u16::from_le_bytes(b2);
        if pending_count(q_size, self.last_avail, avail_idx)? == 0 {
            return None;
        }
        let ring_gpa = avail_entry_gpa(qcfg.avail_gpa, self.last_avail, q_size)?;
        if crate::vmm::read_guest_memory(vm_id, ring_gpa, &mut b2) != 2 {
            return None;
        }
        self.last_avail = self.last_avail.wrapping_add(1);
        let head = u16::from_le_bytes(b2);
        Some((head, read_descriptor_chain(vm_id, qcfg, usize::from(head))))
    }

    /// Publish chain `head` as used with `len` bytes written.
    pub fn publish(&mut self, vm_id: usize, qcfg: &QueueCfg, head: u16, len: u32) -> bool {
        let q_size = usize::from(qcfg.num);
        let (Some(elem_gpa), Some(used_idx_gpa)) = (
            used_entry_gpa(qcfg.used_gpa, self.used, q_size),
            checked_gpa(qcfg.used_gpa, 2, 2),
        ) else {
            return false;
        };
        let mut elem = [0u8; 8];
        elem[0..4].copy_from_slice(&u32::from(head).to_le_bytes());
        elem[4..8].copy_from_slice(&len.to_le_bytes());
        if crate::vmm::write_guest_memory(vm_id, elem_gpa, &elem) != 8 {
            return false;
        }
        let next = self.used.wrapping_add(1);
        if crate::vmm::write_guest_memory(vm_id, used_idx_gpa, &next.to_le_bytes()) != 2 {
            return false;
        }
        self.used = next;
        true
    }
}

/// Copy `bytes` into the device-writable descriptors of a chain, in order;
/// how many went in.
pub fn write_writable(vm_id: usize, bufs: &[DescBuf], bytes: &[u8]) -> usize {
    let mut done = 0;
    for buf in bufs.iter().filter(|buf| buf.writable) {
        let n = (buf.len as usize).min(bytes.len() - done);
        if n == 0 {
            break;
        }
        if crate::vmm::write_guest_memory(vm_id, buf.gpa, &bytes[done..done + n]) != n {
            break;
        }
        done += n;
    }
    done
}

/// Read the device-readable bytes of a chain from `offset` on into `dst`, as
/// one stream across descriptors; how many were read.
pub fn read_readable(vm_id: usize, bufs: &[DescBuf], offset: usize, dst: &mut [u8]) -> usize {
    let mut skip = offset;
    let mut done = 0;
    for buf in bufs.iter().filter(|buf| !buf.writable) {
        let len = buf.len as usize;
        if skip >= len {
            skip -= len;
            continue;
        }
        let n = (len - skip).min(dst.len() - done);
        if n == 0 {
            break;
        }
        let gpa = buf.gpa + skip as u64;
        if crate::vmm::read_guest_memory(vm_id, gpa, &mut dst[done..done + n]) != n {
            break;
        }
        done += n;
        skip = 0;
    }
    done
}

/// Total device-readable and device-writable bytes of a chain.
pub fn lengths(bufs: &[DescBuf]) -> (u64, u64) {
    bufs.iter().fold((0, 0), |(read, write), buf| {
        if buf.writable {
            (read, write + u64::from(buf.len))
        } else {
            (read + u64::from(buf.len), write)
        }
    })
}
