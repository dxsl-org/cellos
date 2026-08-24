//! Bounded Kernel-Owned IPC Wire Message
//!
//! Enforces that all inter-domain IPC payloads are copied into kernel-owned wire buffers
//! before publication to the receiver's queue. Endpoints never retain raw peer pointers
//! or direct access to peer address space pages.
//!
//! Allocations and deallocations for wire messages are explicitly performed under cell 0
//! (kernel ownership) so that cross-cell IPC deliveries and task terminations never corrupt
//! per-cell quota accounting.

use alloc::boxed::Box;
use alloc::vec::Vec;

/// Maximum payload bytes for one bounded IPC message. Derived from the
/// public wire ABI (`api::ipc::IPC_BUF_SIZE`): one queue record never admits
/// more than the documented user-facing buffer.
pub const MAX_IPC_WIRE_PAYLOAD: usize = api::ipc::IPC_BUF_SIZE;

/// Header containing scalar sender identity, generation, and a unique
/// monotonic delivery token. The delivery token disambiguates multiple
/// in-flight messages from the same sender so a blocked sender is woken
/// only when the message it is blocked on is actually consumed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct IpcWireHeader {
    pub sender_tid: usize,
    pub sender_cell_id: u64,
    pub sender_generation: u64,
    pub delivery_id: u64,
}

/// Owned kernel buffer representing an in-flight IPC message. The payload is
/// a `Box<[u8]>`: exactly the bounded wire length, no spare capacity.
#[derive(Debug, PartialEq, Eq)]
pub struct IpcWireMessage {
    pub header: IpcWireHeader,
    payload: Box<[u8]>,
}

impl IpcWireMessage {
    /// Create a wire message from header and raw payload slice.
    /// Allocation is strictly kernel-owned (cell 0).
    /// Fails if payload length exceeds `MAX_IPC_WIRE_PAYLOAD` or allocation fails.
    pub fn try_new(header: IpcWireHeader, payload: &[u8]) -> Result<Self, ()> {
        if payload.len() > MAX_IPC_WIRE_PAYLOAD {
            return Err(());
        }
        let previous_cell = super::hart_local::current_cell_id();
        super::hart_local::set_current_cell_id(0);
        let result = (|| {
            let mut vec = Vec::new();
            vec.try_reserve_exact(payload.len()).map_err(|_| ())?;
            vec.extend_from_slice(payload);
            Ok(vec.into_boxed_slice())
        })();
        super::hart_local::set_current_cell_id(previous_cell);

        result.map(|payload| Self { header, payload })
    }

    /// Length of payload in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.payload.len()
    }

    /// Whether payload is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }

    /// Payload byte slice.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.payload
    }

    /// Fallibly duplicate this message. Unlike `Clone`, allocation failure is
    /// reported instead of aborting the kernel via the non-returning OOM path.
    pub fn try_clone(&self) -> Result<Self, ()> {
        let previous_cell = super::hart_local::current_cell_id();
        super::hart_local::set_current_cell_id(0);
        let result = (|| {
            let mut vec = Vec::new();
            vec.try_reserve_exact(self.payload.len()).map_err(|_| ())?;
            vec.extend_from_slice(&self.payload);
            Ok(vec.into_boxed_slice())
        })();
        super::hart_local::set_current_cell_id(previous_cell);
        result.map(|payload| Self {
            header: self.header,
            payload,
        })
    }
}

impl Drop for IpcWireMessage {
    fn drop(&mut self) {
        let previous_cell = super::hart_local::current_cell_id();
        super::hart_local::set_current_cell_id(0);
        let owned = core::mem::take(&mut self.payload);
        drop(owned);
        super::hart_local::set_current_cell_id(previous_cell);
    }
}
