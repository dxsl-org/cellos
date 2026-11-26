//! The `AUDIO` service (`wire::audio`): stream ownership and client PCM
//! intake. Syscall-free so host tests drive it; `main.rs` owns the receive
//! loop and supplies the Grant calls through [`Grants`].
//!
//! Ownership: the stream belongs to the tid that sent the accepted OPEN, as
//! the kernel reported the sender. A different sender's OPEN is refused with
//! `STATUS_IN_USE` while the holder still owns the Grant it opened with; a
//! Grant that no longer resolves as the holder's means the holder exited (the
//! kernel drops a dead task's Grants) or withdrew it, so the stream is taken
//! over. This needs no owner watch: the probe runs only when someone else
//! wants the stream.

use crate::pcm::PcmQueue;
use wire::audio::{
    AudioOpen, AudioReply, AudioWrite, CLOSE, MAX_CHUNK, OPEN, STATUS_BUSY, STATUS_DEVICE,
    STATUS_INVALID, STATUS_IN_USE, STATUS_NOT_OPEN, STATUS_OK, WRITE,
};

/// The kernel Grant calls the service needs.
pub trait Grants {
    /// The length of `grant` when `owner` owns it and this cell may read it.
    fn len_owned_by(&mut self, grant: u32, owner: usize) -> Option<usize>;
    /// Copies `dst.len()` bytes from byte `offset` of `owner`'s `grant`.
    fn copy(&mut self, grant: u32, owner: usize, offset: usize, dst: &mut [u8]) -> bool;
}

/// What the device side reports for a reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceView {
    pub healthy: bool,
    /// PCM bytes given to the device and not yet completed.
    pub in_flight: usize,
}

/// A stream holder's session totals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// Bytes accepted for playback.
    pub played: u64,
    /// Bytes refused: queue full or device failed.
    pub dropped: u64,
}

/// A change of stream holder, for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Opened { tid: usize },
    Closed { tid: usize, totals: Totals },
}

/// The result of one message: a reply (calls only) and up to two holder
/// changes (a takeover closes the old holder and opens the new one).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub reply: Option<AudioReply>,
    pub events: [Option<Event>; 2],
}

#[derive(Clone, Copy)]
struct Holder {
    tid: usize,
    grant: u32,
    totals: Totals,
}

pub struct Service {
    holder: Option<Holder>,
    /// Client PCM waiting for the device.
    pub queue: PcmQueue,
}

impl Service {
    pub fn new(queue: PcmQueue) -> Self {
        Self {
            holder: None,
            queue,
        }
    }

    /// Whether a client holds the stream.
    pub fn held(&self) -> bool {
        self.holder.is_some()
    }

    /// Handles the message `sender` delivered in `msg` (zero-padded).
    pub fn handle(
        &mut self,
        msg: &[u8],
        sender: usize,
        grants: &mut impl Grants,
        device: DeviceView,
    ) -> Outcome {
        let mut out = Outcome::default();
        let status = match msg.first().copied() {
            Some(OPEN) => self.open(msg, sender, grants, device, &mut out),
            Some(WRITE) => self.write(msg, sender, grants, device),
            Some(CLOSE) => {
                if self.holder.is_some_and(|holder| holder.tid == sender) {
                    out.events[0] = self.release();
                }
                return out;
            }
            _ => STATUS_INVALID,
        };
        out.reply = Some(AudioReply {
            status,
            queued: (self.queue.len() + device.in_flight) as u32,
        });
        out
    }

    fn release(&mut self) -> Option<Event> {
        let holder = self.holder.take()?;
        Some(Event::Closed {
            tid: holder.tid,
            totals: holder.totals,
        })
    }

    fn open(
        &mut self,
        msg: &[u8],
        sender: usize,
        grants: &mut impl Grants,
        device: DeviceView,
        out: &mut Outcome,
    ) -> u32 {
        let Some(AudioOpen { grant }) = AudioOpen::decode(msg) else {
            return STATUS_INVALID;
        };
        if !device.healthy {
            return STATUS_DEVICE;
        }
        if let Some(holder) = self.holder {
            if holder.tid == sender {
                // Re-open by the holder: rebind the Grant, keep the session.
            } else if grants.len_owned_by(holder.grant, holder.tid).is_some() {
                return STATUS_IN_USE;
            }
        }
        if !grants
            .len_owned_by(grant, sender)
            .is_some_and(|len| len >= MAX_CHUNK as usize)
        {
            return STATUS_INVALID;
        }
        match self.holder.as_mut() {
            Some(holder) if holder.tid == sender => holder.grant = grant,
            _ => {
                out.events[0] = self.release();
                out.events[1] = Some(Event::Opened { tid: sender });
                self.holder = Some(Holder {
                    tid: sender,
                    grant,
                    totals: Totals::default(),
                });
            }
        }
        STATUS_OK
    }

    fn write(
        &mut self,
        msg: &[u8],
        sender: usize,
        grants: &mut impl Grants,
        device: DeviceView,
    ) -> u32 {
        let Some(holder) = self.holder.as_mut().filter(|holder| holder.tid == sender) else {
            return STATUS_NOT_OPEN;
        };
        let Some(AudioWrite { len }) = AudioWrite::decode(msg) else {
            return STATUS_INVALID;
        };
        let len = len as usize;
        if !device.healthy {
            holder.totals.dropped += len as u64;
            return STATUS_DEVICE;
        }
        let Some((first, second)) = self.queue.spans(len) else {
            holder.totals.dropped += len as u64;
            return STATUS_BUSY;
        };
        let split = first.len();
        if !grants.copy(holder.grant, sender, 0, first)
            || !grants.copy(holder.grant, sender, split, second)
        {
            return STATUS_INVALID;
        }
        self.queue.commit(len);
        holder.totals.played += len as u64;
        STATUS_OK
    }
}
