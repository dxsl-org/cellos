//! Receiver-owned storage for deferred IPC delivery.

use super::tcb::HOTSWAP_MSG_QUEUE_DEPTH;
use alloc::vec::Vec;

/// Kernel interrupt producers currently emit four- or nine-byte messages.
/// Keeping them inline avoids entering the heap allocator from timer/IRQ context.
const INLINE_MESSAGE_BYTES: usize = 32;

/// Owned message bytes with explicit quota ownership for heap-backed payloads.
pub enum PendingMsgData {
    Inline {
        len: u8,
        bytes: [u8; INLINE_MESSAGE_BYTES],
    },
    Heap {
        bytes: Vec<u8>,
        allocation_cell: usize,
    },
}

impl PendingMsgData {
    /// Copy bytes into inline storage or a fallibly allocated receiver-owned buffer.
    pub fn try_copy(data: &[u8], allocation_cell: usize) -> Result<Self, ()> {
        if data.len() <= INLINE_MESSAGE_BYTES {
            let mut bytes = [0; INLINE_MESSAGE_BYTES];
            bytes[..data.len()].copy_from_slice(data);
            return Ok(Self::Inline {
                len: data.len() as u8,
                bytes,
            });
        }

        let previous_cell = super::hart_local::current_cell_id();
        super::hart_local::set_current_cell_id(allocation_cell);
        let result = (|| {
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(data.len()).map_err(|_| ())?;
            bytes.extend_from_slice(data);
            Ok(bytes)
        })();
        super::hart_local::set_current_cell_id(previous_cell);

        result.map(|bytes| Self::Heap {
            bytes,
            allocation_cell,
        })
    }

    /// Return an empty inline sentinel. Wire-backed records store this in
    /// `data`; all payload reads must go through `PendingMsg::payload()`.
    pub fn empty() -> Self {
        Self::Inline { len: 0, bytes: [0; INLINE_MESSAGE_BYTES] }
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
            Self::Heap { bytes, .. } => bytes.as_slice(),
        }
    }

    /// Pointer to the first payload byte.
    pub fn as_ptr(&self) -> *const u8 {
        self.as_slice().as_ptr()
    }
}

impl Drop for PendingMsgData {
    fn drop(&mut self) {
        let Self::Heap {
            bytes,
            allocation_cell,
        } = self
        else {
            return;
        };

        let owned = core::mem::take(bytes);
        let previous_cell = super::hart_local::current_cell_id();
        super::hart_local::set_current_cell_id(*allocation_cell);
        drop(owned);
        super::hart_local::set_current_cell_id(previous_cell);
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
    pub wire: Option<super::ipc_wire::IpcWireMessage>,
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
    pub fn wire_header(&self) -> Option<super::ipc_wire::IpcWireHeader> {
        self.wire.as_ref().map(|message| message.header)
    }
}

/// Kernel-owned mailbox container; payload allocations remain receiver-owned.
#[derive(Default)]
pub struct PendingMailbox {
    messages: Vec<PendingMsg>,
}

impl PendingMailbox {
    /// Create a mailbox with enough kernel-owned capacity for interrupt producers.
    pub fn new() -> Self {
        let previous_cell = super::hart_local::current_cell_id();
        super::hart_local::set_current_cell_id(0);
        let messages = Vec::with_capacity(HOTSWAP_MSG_QUEUE_DEPTH);
        super::hart_local::set_current_cell_id(previous_cell);
        Self { messages }
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

    pub fn remove(&mut self, index: usize) -> PendingMsg {
        self.messages.remove(index)
    }

    /// Push without charging mailbox-container growth to an arbitrary cell.
    pub fn try_push(&mut self, message: PendingMsg) -> Result<(), ()> {
        if self.messages.len() == self.messages.capacity() {
            let previous_cell = super::hart_local::current_cell_id();
            super::hart_local::set_current_cell_id(0);
            let result = self.messages.try_reserve(1).map_err(|_| ());
            super::hart_local::set_current_cell_id(previous_cell);
            result?;
        }
        self.messages.push(message);
        Ok(())
    }
}

impl Drop for PendingMailbox {
    fn drop(&mut self) {
        let messages = core::mem::take(&mut self.messages);
        let previous_cell = super::hart_local::current_cell_id();
        super::hart_local::set_current_cell_id(0);
        drop(messages);
        super::hart_local::set_current_cell_id(previous_cell);
    }
}
