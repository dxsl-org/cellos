//! Split-virtqueue device-side ring processor.
//!
//! The VMM is on the device side: it reads the avail ring, walks desc chains,
//! hands buffers to the device model, then writes the used ring.
//! All guest memory accesses go via `crate::vmm` syscall wrappers — never raw
//! pointer dereference from the cell (Law 4).

extern crate alloc;
use alloc::vec::Vec;

use crate::virtio_mmio::QueueCfg;
use ostd::io::println;

/// One segment from a virtqueue descriptor chain.
pub struct DescBuf {
    pub gpa: u64,
    pub len: u32,
    /// true = device writes into this buffer (VRING_DESC_F_WRITE); false = device reads.
    pub writable: bool,
}

const FLAGS_NEXT: u16 = 1;
const FLAGS_WRITE: u16 = 2;
const MAX_CHAIN: usize = 64; // guard against infinite chains

/// Read and fully validate one descriptor chain before exposing it to a backend.
pub(crate) fn read_descriptor_chain(
    vm_id: usize,
    qcfg: &QueueCfg,
    head: usize,
) -> Option<Vec<DescBuf>> {
    let q_size = qcfg.num as usize;
    if !qcfg.ready || !qcfg.is_valid() || !crate::virtqueue_guard::valid_descriptor(head, q_size) {
        return None;
    }

    let mut bufs = Vec::with_capacity(q_size.min(MAX_CHAIN));
    let mut cur = head;
    for _ in 0..q_size.min(MAX_CHAIN) {
        let desc_gpa = crate::virtqueue_guard::descriptor_gpa(qcfg.desc_gpa, cur, q_size)?;
        let mut raw = [0u8; 16];
        if crate::vmm::read_guest_memory(vm_id, desc_gpa, &mut raw) != 16 {
            return None;
        }

        let addr = u64::from_le_bytes([
            raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
        ]);
        let len = u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]);
        let flags = u16::from_le_bytes([raw[12], raw[13]]);
        let next = u16::from_le_bytes([raw[14], raw[15]]) as usize;
        if !crate::virtqueue_guard::valid_descriptor_flags(flags)
            || !crate::virtqueue_guard::valid_payload_range(addr, len)
        {
            return None;
        }

        bufs.push(DescBuf {
            gpa: addr,
            len,
            writable: flags & FLAGS_WRITE != 0,
        });
        if flags & FLAGS_NEXT == 0 {
            return Some(bufs);
        }
        if !crate::virtqueue_guard::valid_descriptor(next, q_size) {
            return None;
        }
        cur = next;
    }
    None
}

/// Process one QueueNotify: drain avail ring → walk desc chains → call `handle` → update used ring.
///
/// `last_avail_idx` and `used_idx` are per-queue device-side counters (NOT in guest memory);
/// the caller (device model) owns them and passes them mutably across calls.
pub fn process_notify<F>(
    vm_id: usize,
    qcfg: &QueueCfg,
    last_avail_idx: &mut u16,
    used_idx: &mut u16,
    mut handle: F,
) -> usize
where
    F: FnMut(&[DescBuf]) -> u32,
{
    let q_size = qcfg.num as usize;
    if !qcfg.ready || !qcfg.is_valid() {
        return 0;
    }

    // Read avail.idx (u16 at avail_ring + 2).
    let Some(avail_idx_gpa) = crate::virtqueue_guard::checked_gpa(qcfg.avail_gpa, 2, 2) else {
        return 0;
    };
    let mut b2 = [0u8; 2];
    if crate::vmm::read_guest_memory(vm_id, avail_idx_gpa, &mut b2) != 2 {
        return 0;
    }
    let avail_idx = u16::from_le_bytes(b2);
    if crate::virtqueue_guard::pending_count(q_size, *last_avail_idx, avail_idx).is_none() {
        // Do not resynchronize device-owned state to a corrupt guest index.
        println("[hv-virtio-host] reject pending-delta");
        return 0;
    }

    let mut published = 0;
    while *last_avail_idx != avail_idx {
        // Read avail.ring[last_avail_idx % q_size] — the desc head index.
        let Some(ring_gpa) =
            crate::virtqueue_guard::avail_entry_gpa(qcfg.avail_gpa, *last_avail_idx, q_size)
        else {
            break;
        };
        if crate::vmm::read_guest_memory(vm_id, ring_gpa, &mut b2) != 2 {
            break;
        }
        let head = u16::from_le_bytes(b2) as usize;
        *last_avail_idx = last_avail_idx.wrapping_add(1);

        // Validate the complete chain before any backend observes its payloads.
        let written = if let Some(bufs) = read_descriptor_chain(vm_id, qcfg, head) {
            handle(&bufs)
        } else {
            println("[hv-virtio-host] reject descriptor-chain");
            0
        };

        // Write used ring entry { id: u32, len: u32 } at used.ring[used_idx % q_size].
        let Some(elem_gpa) =
            crate::virtqueue_guard::used_entry_gpa(qcfg.used_gpa, *used_idx, q_size)
        else {
            break;
        };
        let mut elem = [0u8; 8];
        elem[0..4].copy_from_slice(&(head as u32).to_le_bytes());
        elem[4..8].copy_from_slice(&written.to_le_bytes());
        if crate::vmm::write_guest_memory(vm_id, elem_gpa, &elem) != 8 {
            break;
        }

        // Advance used.idx with a store that the guest will see (TCG is SC).
        let next_used = used_idx.wrapping_add(1);
        let Some(used_idx_gpa) = crate::virtqueue_guard::checked_gpa(qcfg.used_gpa, 2, 2) else {
            break;
        };
        if crate::vmm::write_guest_memory(vm_id, used_idx_gpa, &next_used.to_le_bytes()) != 2 {
            break;
        }
        *used_idx = next_used;
        published += 1;
    }
    published
}
