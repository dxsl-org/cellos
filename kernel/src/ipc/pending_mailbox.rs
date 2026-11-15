//! Receiver-owned storage for deferred IPC delivery.

use alloc::vec::Vec;

/// Kernel interrupt producers currently emit four- or nine-byte messages.
/// Keeping them inline avoids entering the heap allocator from timer/IRQ context.
const INLINE_MESSAGE_BYTES: usize = 32;

/// Owned message bytes; a heap-backed payload is charged to the receiver.
pub enum PendingMsgData {
    Inline {
        len: u8,
        bytes: [u8; INLINE_MESSAGE_BYTES],
    },
    Heap {
        bytes: Vec<u8>,
    },
}

impl PendingMsgData {
    /// Copy bytes into inline storage, or into a buffer charged to the
    /// receiving incarnation `receiver` (refused past its quota).
    pub(crate) fn try_copy(
        data: &[u8],
        receiver: crate::memory::cell_quota::Charge,
    ) -> Result<Self, ()> {
        if data.len() <= INLINE_MESSAGE_BYTES {
            let mut bytes = [0; INLINE_MESSAGE_BYTES];
            bytes[..data.len()].copy_from_slice(data);
            return Ok(Self::Inline {
                len: data.len() as u8,
                bytes,
            });
        }

        crate::memory::heap::alloc_for(receiver, || {
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(data.len()).map_err(|_| ())?;
            bytes.extend_from_slice(data);
            Ok(Self::Heap { bytes })
        })
    }

    /// Return an empty inline sentinel. Wire-backed records store this in
    /// `data`; all payload reads must go through `PendingMsg::payload()`.
    pub fn empty() -> Self {
        Self::Inline {
            len: 0,
            bytes: [0; INLINE_MESSAGE_BYTES],
        }
    }

    /// Number of payload bytes.
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    /// Whether the payload is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Borrow the payload bytes.
    pub fn as_slice(&self) -> &[u8] {
        match self {
            Self::Inline { len, bytes } => &bytes[..*len as usize],
            Self::Heap { bytes } => bytes.as_slice(),
        }
    }

    /// Pointer to the first payload byte.
    pub fn as_ptr(&self) -> *const u8 {
        self.as_slice().as_ptr()
    }
}

/// A message buffered until the receiver resumes in its own syscall context.
///
/// Interrupt producers use `data` (inline, allocation-free). Cross-task IPC
/// senders publish a kernel-owned `wire` message carrying scalar sender
/// identity/generation so queue records never alias peer memory.
pub struct PendingMsg {
    pub sender_tid: usize,
    pub data: PendingMsgData,
    pub wire: Option<crate::ipc::ipc_wire::IpcWireMessage>,
    pub enqueued_tick: u64,
}

impl PendingMsg {
    /// Payload bytes, preferring the kernel-owned wire buffer.
    pub fn payload(&self) -> &[u8] {
        match &self.wire {
            Some(message) => message.as_slice(),
            None => self.data.as_slice(),
        }
    }

    /// Scalar sender identity carried by the wire header, if published via IPC.
    pub fn wire_header(&self) -> Option<crate::ipc::ipc_wire::IpcWireHeader> {
        self.wire.as_ref().map(|message| message.header)
    }
}

/// Kernel-owned mailbox container; payload allocations remain receiver-owned.
#[derive(Default)]
pub struct PendingMailbox {
    messages: Vec<PendingMsg>,
}

impl PendingMailbox {
    /// Create an empty mailbox.
    ///
    /// The container is deliberately *not* pre-reserved: `HOTSWAP_MSG_QUEUE_DEPTH`
    /// records cost 6 656 bytes per task, and a parked cell never receives one, so
    /// the reservation was pure per-cell overhead. It was also the allocation that
    /// halted the capacity sweep at ~150 cells, because `Vec::with_capacity` is
    /// infallible. `try_push` reserves fallibly on first use, charged to the
    /// kernel — the same path the overflow case already takes.
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn as_slice(&self) -> &[PendingMsg] {
        self.messages.as_slice()
    }

    pub fn iter(&self) -> core::slice::Iter<'_, PendingMsg> {
        self.messages.iter()
    }

    #[cfg(feature = "p01-trace")]
    pub(crate) fn observe_owner(&self, owner: crate::p01_measurement::Owner) {
        crate::memory::heap::p01_owner::runtime_vec(
            &self.messages,
            owner,
            crate::memory::heap::p01_owner::MAILBOX,
        );
    }
    #[cfg(feature = "p01-trace")]
    pub(crate) fn observe_wire_commit(&self, owner: crate::p01_measurement::Owner) {
        if let Some(wire) = self
            .messages
            .last()
            .and_then(|message| message.wire.as_ref())
        {
            wire.observe_queue_commit(owner);
        }
    }

    pub fn remove(&mut self, index: usize) -> PendingMsg {
        self.messages.remove(index)
    }

    /// Remove the record a receive snapshot delivered at `pos`, once its
    /// copy-out succeeded. Returns the removed record.
    ///
    /// The snapshot and this commit are separate `SCHEDULER` holds, and another
    /// path (cancel, expiry, owner death) may remove a record ahead of `pos`
    /// in between. A wire is therefore found by its unique `delivery_id`;
    /// only an inline record without a wire falls back to `pos + sender_tid`.
    /// A wire that is no longer queued is never guessed at.
    pub fn remove_delivered(
        &mut self,
        pos: usize,
        sender_tid: usize,
        delivery_id: Option<u64>,
    ) -> Option<PendingMsg> {
        let index = match delivery_id {
            Some(id) => self.messages.iter().position(|message| {
                message.sender_tid == sender_tid
                    && message
                        .wire
                        .as_ref()
                        .is_some_and(|wire| wire.header.delivery_id == id)
            })?,
            None => {
                let record = self
                    .messages
                    .get(pos)
                    .filter(|message| message.sender_tid == sender_tid)?;
                // A receive arm that dropped a wire's delivery id would leave
                // the wire queued for a second delivery. Only inline records
                // legitimately have none.
                debug_assert!(
                    record.wire.is_none(),
                    "a wire was committed without its delivery id"
                );
                record.wire.is_none().then_some(pos)?
            }
        };
        Some(self.messages.remove(index))
    }

    /// Push `message`. Container growth is kernel memory: the depth bound caps
    /// it, and a kernel relay to a receiver at its quota is never refused for it.
    pub(crate) fn try_push(&mut self, message: PendingMsg) -> Result<(), ()> {
        if self.messages.len() == self.messages.capacity() {
            crate::memory::heap::alloc_for(crate::memory::cell_quota::Charge::KERNEL, || {
                self.messages.try_reserve(1)
            })
            .map_err(|_| ())?;
        }
        self.messages.push(message);
        Ok(())
    }
}
