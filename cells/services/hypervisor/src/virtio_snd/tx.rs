//! The output stream's tx side: buffers are queued when the guest posts them
//! and completed when the playback clock has played past their last byte.
//!
//! A tx chain is a `virtio_snd_pcm_xfer` header (stream id), the PCM bytes,
//! then a device-writable `virtio_snd_pcm_status` (§5.14.6.8). Its PCM is
//! copied into the host queue at once, so forwarding runs ahead of playback;
//! only the completion waits. Anything malformed, or posted while the stream
//! cannot take I/O, completes at once with `S_IO_ERR`.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use ostd::io::println;

use super::clock::PlayClock;
use super::control::{Effect, Pcm, FRAME_BYTES, MAX_BUFFER_BYTES, S_IO_ERR, S_OK};
use super::host_queue::HostQueue;
use super::ring::{lengths, read_readable, write_writable, Ring};
use crate::virtio_mmio::QueueCfg;
use crate::virtqueue::DescBuf;

const XFER_LEN: u64 = 4;
const STATUS_LEN: u64 = 8;
/// Buffers held at once: a ring's worth. A guest re-posting heads it has not
/// had back cannot grow the list past it.
const MAX_PENDING: usize = crate::virtqueue_guard::MAX_QUEUE_SIZE;
const COPY_CHUNK: usize = 4096;

/// A queued tx buffer: its head, its status descriptors and where its last
/// byte lies in the stream.
struct Pending {
    head: u16,
    status: Vec<DescBuf>,
    end: u64,
}

pub struct Output {
    pub pcm: Pcm,
    clock: PlayClock,
    /// Stream bytes queued since the last release; every pending end lies
    /// at or before it.
    queued_end: u64,
    pending: VecDeque<Pending>,
    ring: Ring,
    /// The txq's layout. It is fixed from DRIVER_OK until reset (queue reset
    /// is not offered), so completions after the notify may use it.
    cfg: QueueCfg,
    pub host: HostQueue,
    /// STARTs so far.
    pub starts: u32,
}

impl Output {
    pub fn new() -> Self {
        Self {
            pcm: Pcm::new(),
            clock: PlayClock::new(),
            queued_end: 0,
            pending: VecDeque::new(),
            ring: Ring::default(),
            cfg: QueueCfg::default(),
            host: HostQueue::new(),
            starts: 0,
        }
    }

    /// Take every chain the guest posted. Whether used entries were published
    /// (only refused chains complete here).
    pub fn notify(&mut self, vm_id: usize, qcfg: &QueueCfg, now_ns: u64) -> bool {
        self.cfg = *qcfg;
        let mut published = false;
        while let Some((head, chain)) = self.ring.pop(vm_id, qcfg) {
            published |= match chain {
                Some(bufs) => self.queue(vm_id, head, bufs, now_ns),
                None => self.ring.publish(vm_id, qcfg, head, 0),
            };
        }
        published
    }

    fn queue(&mut self, vm_id: usize, head: u16, bufs: Vec<DescBuf>, now_ns: u64) -> bool {
        let (readable, writable) = lengths(&bufs);
        let payload = readable.saturating_sub(XFER_LEN);
        let mut stream = [0u8; XFER_LEN as usize];
        let valid = readable >= XFER_LEN
            && writable >= STATUS_LEN
            && payload.is_multiple_of(u64::from(FRAME_BYTES))
            && payload <= u64::from(MAX_BUFFER_BYTES)
            && self.pcm.accepts_io()
            && self.pending.len() < MAX_PENDING
            && read_readable(vm_id, &bufs, 0, &mut stream) == stream.len()
            && u32::from_le_bytes(stream) == 0;
        if !valid {
            return self.complete(vm_id, head, &bufs, S_IO_ERR, 0);
        }
        self.forward(vm_id, &bufs, payload as usize);
        // A stream that ran dry plays this buffer from now.
        self.clock.advance(now_ns, self.queued_end);
        self.queued_end += payload;
        let status = bufs.into_iter().filter(|buf| buf.writable).collect();
        self.pending.push_back(Pending {
            head,
            status,
            end: self.queued_end,
        });
        false
    }

    /// Copy what the host queue has room for; count the rest dropped.
    fn forward(&mut self, vm_id: usize, bufs: &[DescBuf], payload: usize) {
        let take = payload.min(self.host.room());
        let mut chunk = [0u8; COPY_CHUNK];
        let mut done = 0;
        while done < take {
            let n = (take - done).min(COPY_CHUNK);
            let offset = XFER_LEN as usize + done;
            if read_readable(vm_id, bufs, offset, &mut chunk[..n]) != n {
                break;
            }
            self.host.push(&chunk[..n]);
            done += n;
        }
        self.host.count_dropped((payload - done) as u64);
    }

    /// Complete the buffers the clock has played by `now_ns`.
    pub fn poll(&mut self, vm_id: usize, now_ns: u64) -> bool {
        if self.pending.is_empty() || !self.pcm.playing() {
            return false;
        }
        let played = self.clock.advance(now_ns, self.queued_end);
        self.complete_until(vm_id, played)
    }

    /// Act on a control request's effect. Whether tx entries were published.
    pub fn apply(&mut self, effect: Effect, vm_id: usize, now_ns: u64) -> bool {
        match effect {
            Effect::None => false,
            Effect::Start => {
                self.clock.start(now_ns);
                self.starts = self.starts.saturating_add(1);
                println("[hv-x86] sound: stream started");
                false
            }
            Effect::Stop => {
                self.clock.stop(now_ns, self.queued_end);
                println("[hv-x86] sound: stream stopped");
                let played = self.clock.advance(now_ns, self.queued_end);
                self.complete_until(vm_id, played)
            }
            // Every pending buffer completes before the RELEASE reply
            // (§5.14.6.6.5.1); the stream's position starts over.
            Effect::Release => {
                let published = self.complete_until(vm_id, u64::MAX);
                self.clock.reset();
                self.queued_end = 0;
                self.host.clear();
                published
            }
        }
    }

    /// Complete, oldest first, the pending buffers that end by `played`.
    fn complete_until(&mut self, vm_id: usize, played: u64) -> bool {
        let latency = self
            .queued_end
            .saturating_sub(played)
            .min(u64::from(u32::MAX)) as u32;
        let mut published = false;
        while self
            .pending
            .front()
            .is_some_and(|front| front.end <= played)
        {
            let Some(done) = self.pending.pop_front() else {
                break;
            };
            published |= self.complete(vm_id, done.head, &done.status, S_OK, latency);
        }
        published
    }

    fn complete(
        &mut self,
        vm_id: usize,
        head: u16,
        bufs: &[DescBuf],
        status: u32,
        latency: u32,
    ) -> bool {
        let mut bytes = [0u8; STATUS_LEN as usize];
        bytes[..4].copy_from_slice(&status.to_le_bytes());
        bytes[4..].copy_from_slice(&latency.to_le_bytes());
        let written = write_writable(vm_id, bufs, &bytes);
        self.ring.publish(vm_id, &self.cfg, head, written as u32)
    }

    /// Device reset: the guest's rings are gone, so nothing is written back.
    pub fn reset(&mut self) {
        if self.pcm.playing() {
            println("[hv-x86] sound: stream stopped");
        }
        self.pcm.reset();
        self.clock.reset();
        self.queued_end = 0;
        self.pending.clear();
        self.ring = Ring::default();
        self.cfg = QueueCfg::default();
        self.host.clear();
    }
}
