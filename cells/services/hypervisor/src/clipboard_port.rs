//! The clipboard side of the guest's virtio-console (x86 Chrome guest): its
//! transmitq carries the guest's clipboard lines out, its receiveq the host's
//! in (`clipboard_line`). Device-side state only; the launcher end is
//! `client_clipboard`, on the BSP's thread.
//!
//! Only the newest text matters either way, so both directions hold at most
//! one waiting text: memory stays bounded whatever the guest or the launcher
//! does. A line already partly in the guest finishes before a newer one
//! starts, so the guest never sees two texts spliced into one line.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::clipboard_line::{encode_line, LineDecoder};
use crate::virtio_mmio::QueueCfg;
use crate::virtqueue::read_descriptor_chain;
use crate::virtqueue_guard::{avail_entry_gpa, checked_gpa, pending_count, used_entry_gpa};

#[derive(Default)]
pub struct ClipboardPort {
    decoder: LineDecoder,
    /// The newest text the guest copied, not yet taken by the BSP's thread.
    from_guest: Option<String>,
    /// The line being delivered into the receiveq, and how much of it the
    /// guest already has.
    to_guest: Vec<u8>,
    sent: usize,
    /// A newer line, waiting for `to_guest` to finish.
    next: Option<Vec<u8>>,
}

impl ClipboardPort {
    pub const fn new() -> Self {
        Self {
            decoder: LineDecoder::new(),
            from_guest: None,
            to_guest: Vec::new(),
            sent: 0,
            next: None,
        }
    }

    /// Bytes the guest wrote to its console.
    pub fn tx(&mut self, bytes: &[u8]) {
        if let Some(text) = self.decoder.feed(bytes) {
            self.from_guest = Some(text);
        }
    }

    /// The newest text the guest copied since the last call.
    pub fn take_from_guest(&mut self) -> Option<String> {
        self.from_guest.take()
    }

    /// Queue `text` for the guest; it replaces any text not yet started.
    pub fn queue_for_guest(&mut self, text: &str) {
        let line = encode_line(text);
        if self.sent == 0 {
            self.to_guest = line;
            self.next = None;
        } else {
            self.next = Some(line);
        }
    }

    /// The guest reset the device: a half-read line is lost on its side, so
    /// it is sent again whole; a half-written one is dropped.
    pub fn reset(&mut self) {
        self.decoder.reset();
        self.sent = 0;
    }

    /// Write queued bytes into the receiveq's guest-writable buffers, one
    /// used entry per chain, while both last. `last_avail`/`used` are the
    /// device's ring counters for the queue. Returns whether any entry was
    /// published.
    pub fn fill_rx(
        &mut self,
        qcfg: &QueueCfg,
        vm_id: usize,
        last_avail: &mut u16,
        used: &mut u16,
    ) -> bool {
        if !qcfg.ready || !qcfg.is_valid() || self.sent == self.to_guest.len() {
            return false;
        }
        let q_size = usize::from(qcfg.num);
        let (Some(avail_idx_gpa), Some(used_idx_gpa)) = (
            checked_gpa(qcfg.avail_gpa, 2, 2),
            checked_gpa(qcfg.used_gpa, 2, 2),
        ) else {
            return false;
        };
        let mut b2 = [0u8; 2];
        if crate::vmm::read_guest_memory(vm_id, avail_idx_gpa, &mut b2) != 2 {
            return false;
        }
        let avail_idx = u16::from_le_bytes(b2);
        // Never follow a corrupt guest index (as `virtqueue::process_notify`).
        if pending_count(q_size, *last_avail, avail_idx).is_none() {
            return false;
        }
        let mut published = false;
        while *last_avail != avail_idx && self.sent < self.to_guest.len() {
            let Some(ring_gpa) = avail_entry_gpa(qcfg.avail_gpa, *last_avail, q_size) else {
                break;
            };
            if crate::vmm::read_guest_memory(vm_id, ring_gpa, &mut b2) != 2 {
                break;
            }
            let head = u16::from_le_bytes(b2);
            *last_avail = last_avail.wrapping_add(1);
            let written = read_descriptor_chain(vm_id, qcfg, usize::from(head)).map_or(0, |bufs| {
                let mut written = 0;
                for buf in bufs.iter().filter(|buf| buf.writable) {
                    written += self.write_into(vm_id, buf.gpa, buf.len as usize);
                }
                written
            });
            let Some(elem_gpa) = used_entry_gpa(qcfg.used_gpa, *used, q_size) else {
                break;
            };
            let mut elem = [0u8; 8];
            elem[0..4].copy_from_slice(&u32::from(head).to_le_bytes());
            elem[4..8].copy_from_slice(&(written as u32).to_le_bytes());
            if crate::vmm::write_guest_memory(vm_id, elem_gpa, &elem) != 8 {
                break;
            }
            let next_used = used.wrapping_add(1);
            if crate::vmm::write_guest_memory(vm_id, used_idx_gpa, &next_used.to_le_bytes()) != 2 {
                break;
            }
            *used = next_used;
            published = true;
        }
        published
    }

    /// Copy the next queued bytes into one guest buffer of `len` bytes at
    /// `gpa`; how many went in. A finished line makes room for the next.
    fn write_into(&mut self, vm_id: usize, gpa: u64, len: usize) -> usize {
        let rest = &self.to_guest[self.sent..];
        let n = rest.len().min(len);
        if n == 0 || crate::vmm::write_guest_memory(vm_id, gpa, &rest[..n]) != n {
            return 0;
        }
        self.sent += n;
        if self.sent == self.to_guest.len() {
            self.to_guest = self.next.take().unwrap_or_default();
            self.sent = 0;
        }
        n
    }
}
