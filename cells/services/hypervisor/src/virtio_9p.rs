//! virtio-9p device model (DeviceID 9): the Chrome guest's Downloads share.
//!
//! x86 slot 5 on the shared display IRQ; one request queue. Each chain is a
//! 9P request in its device-readable buffers and room for the reply in its
//! device-writable ones. Requests are served to completion inside the queue
//! notify, on the vCPU thread that rang it, like `virtio_blk`: the VFS calls
//! block that vCPU only.

extern crate alloc;
use alloc::vec;

use crate::p9::{Server, VfsShare};
use crate::virtio_blk::{gather_from_guest, scatter_to_guest};
use crate::virtio_mmio::{QueueCfg, VirtioDevice};
use crate::virtqueue::{process_notify, DescBuf};

/// The mount tag the guest names the share by.
const MOUNT_TAG: &[u8] = b"downloads";
/// VIRTIO_9P_MOUNT_TAG: the config space holds the tag.
const F_MOUNT_TAG: u32 = 1;
/// Rlerror, whose replies count as errors.
const RLERROR: u8 = crate::p9::msg::RLERROR;
/// Errors logged per session.
const LOGGED_ERRORS: u64 = 16;

pub struct P9Dev {
    server: Server<VfsShare>,
    last_avail: u16,
    used_idx: u16,
    requests: u64,
    errors: u64,
}

impl P9Dev {
    pub fn new() -> Self {
        Self {
            server: Server::new(VfsShare::new()),
            last_avail: 0,
            used_idx: 0,
            requests: 0,
            errors: 0,
        }
    }

    /// Close the guest's session (syncing what it wrote); the requests
    /// served and how many of them were answered with an error.
    pub fn end_session(&mut self) -> (u64, u64) {
        self.server.reset();
        (self.requests, self.errors)
    }

    /// Serve one chain; the bytes written into its writable buffers.
    fn serve(&mut self, vm_id: usize, bufs: &[DescBuf]) -> u32 {
        let split = bufs
            .iter()
            .position(|buf| buf.writable)
            .unwrap_or(bufs.len());
        let (request, reply) = bufs.split_at(split);
        if reply.iter().any(|buf| !buf.writable) {
            return 0;
        }
        let sum = |bufs: &[DescBuf]| bufs.iter().map(|buf| buf.len as usize).sum::<usize>();
        let (room, offered) = (sum(reply), sum(request));
        // A request longer than msize is read only that far: the server
        // refuses it from its size field.
        let mut msg = vec![0u8; offered.min(self.server.msize())];
        if !gather_from_guest(vm_id, request, 0, &mut msg) {
            return 0;
        }
        let answer = self.server.handle(&msg, room);
        self.requests += 1;
        if answer.get(4) == Some(&RLERROR) {
            self.errors += 1;
            // The first few, so a guest that cannot save says why (request type, errno).
            if self.errors <= LOGGED_ERRORS {
                let errno = answer
                    .get(7..11)
                    .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                ostd::io::println(&alloc::format!(
                    "[hv-x86] 9p error: request type {} errno {}",
                    msg.get(4).copied().unwrap_or(0),
                    errno
                ));
            }
        }
        if answer.len() > room || !scatter_to_guest(vm_id, reply, 0, &answer) {
            return 0;
        }
        answer.len() as u32
    }
}

impl VirtioDevice for P9Dev {
    fn device_id(&self) -> u32 {
        9
    }

    fn device_features_lo(&self) -> u32 {
        F_MOUNT_TAG
    }

    /// `tag_len: u16` (little-endian), then the tag.
    fn config_read(&self, offset: usize) -> u32 {
        let mut word = [0u8; 4];
        for (i, byte) in word.iter_mut().enumerate() {
            *byte = match offset + i {
                0 => MOUNT_TAG.len() as u8,
                1 => 0,
                at => MOUNT_TAG.get(at - 2).copied().unwrap_or(0),
            };
        }
        u32::from_le_bytes(word)
    }

    fn notify(&mut self, q: usize, qcfg: &QueueCfg, vm_id: usize, _vcpu_id: usize) -> bool {
        if q != 0 {
            return false;
        }
        let (mut last_avail, mut used_idx) = (self.last_avail, self.used_idx);
        let published = process_notify(vm_id, qcfg, &mut last_avail, &mut used_idx, |bufs| {
            self.serve(vm_id, bufs)
        });
        (self.last_avail, self.used_idx) = (last_avail, used_idx);
        published > 0
    }

    /// A reset ends the guest's session: every fid and open file goes.
    fn reset(&mut self) {
        self.last_avail = 0;
        self.used_idx = 0;
        self.server.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_space_is_tag_length_then_tag() {
        let dev = P9Dev::new();
        assert_eq!(dev.config_read(0).to_le_bytes(), [9, 0, b'd', b'o']);
        assert_eq!(dev.config_read(8).to_le_bytes(), [b'a', b'd', b's', 0]);
        assert_eq!(dev.device_id(), 9);
        assert_eq!(dev.device_features_lo(), 1);
    }
}
