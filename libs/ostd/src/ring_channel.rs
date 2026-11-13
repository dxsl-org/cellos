// SPDX-License-Identifier: MPL-2.0
//! User-space abstractions for zero-trap fastpath SPSC Ring Channels.
//!
//! Provides non-blocking, spinning, and yielding send/recv primitives
//! for inter-cell communication within the Cellos SAS.
//!
//! ## What crosses between cells
//! The shared `BiRingChannel` lives in a kernel Grant, and the token a host hands
//! its peer is the Grant id. The peer resolves it through `GrantSlice` with the
//! host's kernel-stamped tid as the expected owner, so the kernel — not the
//! token's bits — decides which memory the client's ring stores land in: a forged
//! token, someone else's Grant, a Grant not shared to the client, or one too
//! short for a channel all resolve to `None`. The channel holds only atomics, so
//! whatever bytes a hostile peer leaves in it are a valid value; the ring ops
//! refuse nonsensical slot lengths instead of trusting them.
//!
//! ## How long the client's view lasts
//! The host's `ChannelHost` owns the Grant and frees it on drop, so a host keeps
//! it alive until its peer is gone. A private-root (domain) client loses the
//! mapping when the Grant is freed (`GrantFree` revokes receivers), so a late
//! access faults inside the client. On the SAS path `GrantFree` also drops the
//! user mapping, but the frames can later be reissued at the same address to
//! another owner — a kernel property every Grant consumer shares, which this API
//! cannot close from user space.
#![allow(unsafe_code)]

use crate::grant::GrantHandle;
use crate::syscall::{
    sys_grant_share, sys_grant_slice_with_len, sys_grant_slice_with_len_owned_by,
};
use core::mem::{align_of, size_of};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, Ordering};
pub use wire::ring_channel::{
    BiRingChannel, RingError, RingMessageMeta, SpscRing, SpscSlot, RING_CAPACITY, RING_SLOT_BYTES,
};

/// Producer half of a single-producer single-consumer ring channel.
pub struct RingSender<'a> {
    ring: &'a SpscRing,
}

impl<'a> RingSender<'a> {
    /// Bind a sender to a ring buffer.
    pub const fn new(ring: &'a SpscRing) -> Self {
        Self { ring }
    }

    /// Enqueue a message immediately, returning `Err(RingError::Full)` if no space.
    pub fn try_send(&self, msg: &[u8], seq: u32, flags: u32) -> Result<(), RingError> {
        self.ring.try_push(msg, seq, flags)
    }

    /// Spin up to `max_spins` attempting to send.
    pub fn send_spin(
        &self,
        msg: &[u8],
        seq: u32,
        flags: u32,
        max_spins: usize,
    ) -> Result<(), RingError> {
        for _ in 0..max_spins {
            match self.try_send(msg, seq, flags) {
                Ok(()) => return Ok(()),
                Err(RingError::Full) => core::hint::spin_loop(),
                Err(e) => return Err(e),
            }
        }
        Err(RingError::Full)
    }

    /// Send a message, spinning briefly then yielding to the scheduler if full.
    pub fn send_blocking(&self, msg: &[u8], seq: u32, flags: u32) -> Result<(), RingError> {
        // Fast spin first (typical when queue is nearly empty)
        if self.send_spin(msg, seq, flags, 32).is_ok() {
            return Ok(());
        }

        // Cooperative yield loop
        loop {
            match self.try_send(msg, seq, flags) {
                Ok(()) => return Ok(()),
                Err(RingError::Full) => crate::task::yield_now(),
                Err(e) => return Err(e),
            }
        }
    }
}

/// Consumer half of a single-producer single-consumer ring channel.
pub struct RingReceiver<'a> {
    ring: &'a SpscRing,
}

impl<'a> RingReceiver<'a> {
    /// Bind a receiver to a ring buffer.
    pub const fn new(ring: &'a SpscRing) -> Self {
        Self { ring }
    }

    /// Dequeue a message immediately, returning `Ok(None)` if empty.
    pub fn try_recv(&self, out: &mut [u8]) -> Result<Option<RingMessageMeta>, RingError> {
        self.ring.try_pop(out)
    }

    /// Spin up to `max_spins` waiting for a message.
    pub fn recv_spin(
        &self,
        out: &mut [u8],
        max_spins: usize,
    ) -> Result<Option<RingMessageMeta>, RingError> {
        for _ in 0..max_spins {
            match self.try_recv(out) {
                Ok(Some(meta)) => return Ok(Some(meta)),
                Ok(None) => core::hint::spin_loop(),
                Err(e) => return Err(e),
            }
        }
        Ok(None)
    }

    /// Dequeue a message, spinning briefly then yielding to the scheduler if empty.
    pub fn recv_blocking(&self, out: &mut [u8]) -> Result<RingMessageMeta, RingError> {
        // Fast spin first
        if let Ok(Some(meta)) = self.recv_spin(out, 32) {
            return Ok(meta);
        }

        // Cooperative yield loop
        loop {
            match self.try_recv(out) {
                Ok(Some(meta)) => return Ok(meta),
                Ok(None) => crate::task::yield_now(),
                Err(e) => return Err(e),
            }
        }
    }
}

/// A bi-directional zero-trap RPC endpoint communicating over a shared `BiRingChannel`.
pub struct FastpathEndpoint<'a> {
    pub tx: RingSender<'a>,
    pub rx: RingReceiver<'a>,
    seq: AtomicU32,
}

impl<'a> FastpathEndpoint<'a> {
    /// Initialize Endpoint A (transmits on `a_to_b`, receives on `b_to_a`).
    pub const fn new_endpoint_a(channel: &'a BiRingChannel) -> Self {
        Self {
            tx: RingSender::new(&channel.a_to_b),
            rx: RingReceiver::new(&channel.b_to_a),
            seq: AtomicU32::new(1),
        }
    }

    /// Initialize Endpoint B (transmits on `b_to_a`, receives on `a_to_b`).
    pub const fn new_endpoint_b(channel: &'a BiRingChannel) -> Self {
        Self {
            tx: RingSender::new(&channel.b_to_a),
            rx: RingReceiver::new(&channel.a_to_b),
            seq: AtomicU32::new(1),
        }
    }

    /// Send a request and wait for the matching reply directly through shared memory.
    /// Completely zero-trap, zero-syscall on the fastpath.
    pub fn call(&self, req: &[u8], resp_buf: &mut [u8]) -> Result<usize, RingError> {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        self.tx.send_blocking(req, seq, 0)?;

        let meta = self.rx.recv_blocking(resp_buf)?;
        Ok(meta.len)
    }
}

/// `GrantShare` permission for a peer that both reads and writes the rings.
const GRANT_PERM_READ_WRITE: u8 = 2;

/// View a kernel-resolved Grant mapping as a channel; `None` when the mapping is
/// null, misaligned, or too short to hold a whole `BiRingChannel`.
fn channel_in_grant(ptr: *mut u8, len: usize) -> Option<NonNull<BiRingChannel>> {
    if len < size_of::<BiRingChannel>()
        || !(ptr as usize).is_multiple_of(align_of::<BiRingChannel>())
    {
        return None;
    }
    NonNull::new(ptr.cast())
}

/// Host that owns a shared `BiRingChannel` placed in a kernel Grant.
pub struct ChannelHost {
    grant: GrantHandle<u8>,
    channel: NonNull<BiRingChannel>,
}

impl ChannelHost {
    /// Allocate a channel in a fresh Grant owned by this task; `None` when the
    /// kernel refuses the allocation.
    pub fn new() -> Option<Self> {
        let grant = GrantHandle::<u8>::alloc(size_of::<BiRingChannel>())?;
        let (ptr, len) = sys_grant_slice_with_len(grant.id())?;
        let channel = channel_in_grant(ptr, len)?;
        // SAFETY: the fresh Grant is mapped for at least a whole aligned channel
        // and is not shared yet, so nothing else observes this initialising write.
        unsafe { channel.as_ptr().write(BiRingChannel::new()) };
        Some(Self { grant, channel })
    }

    /// Share the channel with `peer_tid` (read-write) and return the token the
    /// peer passes to [`ChannelClient::connect`]; `None` if the kernel refuses.
    pub fn share_with(&self, peer_tid: usize) -> Option<u64> {
        sys_grant_share(self.grant.id(), peer_tid, GRANT_PERM_READ_WRITE)
            .then_some(self.grant.id() as u64)
    }

    /// Get the Endpoint A view for the local cell.
    pub fn endpoint(&self) -> FastpathEndpoint<'_> {
        // SAFETY: `self.grant` keeps the Grant mapped for as long as `self`, which
        // bounds the borrow; `new` initialised the channel, and its atomics are the
        // only state the peer shares.
        FastpathEndpoint::new_endpoint_a(unsafe { self.channel.as_ref() })
    }
}

/// Client attached to a host's channel Grant.
pub struct ChannelClient {
    channel: NonNull<BiRingChannel>,
}

impl ChannelClient {
    /// Attach to the channel Grant `token` that `host_tid` allocated and shared
    /// to this task.
    ///
    /// `host_tid` must be the sender tid the kernel stamped on the message that
    /// carried `token`, never a value from a payload. `None` unless the kernel
    /// resolves `token` as a Grant owned by `host_tid`, accessible to this task,
    /// and long enough for a whole channel.
    pub fn connect(token: u64, host_tid: usize) -> Option<Self> {
        let grant_id = usize::try_from(token).ok()?;
        let (ptr, len) = sys_grant_slice_with_len_owned_by(grant_id, host_tid)?;
        Some(Self {
            channel: channel_in_grant(ptr, len)?,
        })
    }

    /// Get the Endpoint B view for the client cell.
    pub fn endpoint(&self) -> FastpathEndpoint<'_> {
        // SAFETY: `connect` took the pointer from the kernel's validation of a
        // Grant shared to this task and bounded it to a whole aligned channel; an
        // atomics-only channel is valid for any bytes the host leaves in it. The
        // view lasts while the host holds its Grant (see the module note).
        FastpathEndpoint::new_endpoint_b(unsafe { self.channel.as_ref() })
    }
}

#[cfg(test)]
#[path = "ring-channel-tests.rs"]
mod tests;
