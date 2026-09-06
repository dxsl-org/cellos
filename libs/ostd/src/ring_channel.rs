// SPDX-License-Identifier: MPL-2.0
//! User-space abstractions for zero-trap fastpath SPSC Ring Channels.
//!
//! Provides non-blocking, spinning, and yielding send/recv primitives
//! for inter-cell communication within the Cellos SAS.
#![allow(unsafe_code)]

pub use api::ring_channel::{
    BiRingChannel, RingError, RingMessageMeta, SpscRing, SpscSlot, RING_CAPACITY, RING_SLOT_BYTES,
};
use core::sync::atomic::{AtomicU32, Ordering};

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

use alloc::boxed::Box;

/// Host that allocates and owns a shared `BiRingChannel`.
pub struct ChannelHost {
    channel: Box<BiRingChannel>,
}

impl ChannelHost {
    /// Allocate a new channel on the heap.
    pub fn new() -> Self {
        Self {
            channel: Box::new(BiRingChannel::new()),
        }
    }

    /// Obtain the 64-bit address token to send to the peer cell.
    pub fn handle(&self) -> u64 {
        (&*self.channel as *const BiRingChannel) as usize as u64
    }

    /// Get the Endpoint A view for the local cell.
    pub fn endpoint(&self) -> FastpathEndpoint<'_> {
        FastpathEndpoint::new_endpoint_a(&self.channel)
    }
}

impl Default for ChannelHost {
    fn default() -> Self {
        Self::new()
    }
}

/// Client that attaches to an existing `BiRingChannel` via its address token.
pub struct ChannelClient {
    ptr: *const BiRingChannel,
}

impl ChannelClient {
    /// Connect to a channel handle provided by the host cell.
    /// Returns None if the handle is null or misaligned.
    pub fn connect(handle: u64) -> Option<Self> {
        let addr = handle as usize;
        if addr == 0 || addr % core::mem::align_of::<BiRingChannel>() != 0 {
            return None;
        }
        Some(Self {
            ptr: addr as *const BiRingChannel,
        })
    }

    /// Get the Endpoint B view for the client cell.
    pub fn endpoint(&self) -> FastpathEndpoint<'_> {
        // SAFETY: The handle was issued by a live ChannelHost in the shared address space
        // and verified to be non-null and properly aligned.
        let channel = unsafe { &*self.ptr };
        FastpathEndpoint::new_endpoint_b(channel)
    }
}
