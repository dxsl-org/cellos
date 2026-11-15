//! Bounded Kernel-Owned IPC Wire Message
//!
//! Enforces that all inter-domain IPC payloads are copied into kernel-owned wire buffers
//! before publication to the receiver's queue. Endpoints never retain raw peer pointers
//! or direct access to peer address space pages.
//!
//! A wire built from a sender's bytes is charged to the sender incarnation the
//! header names, or to the kernel for a kernel sender (CellId 0). Storage built
//! for anyone else (an async reply buffer, a receive snapshot) names its charge
//! explicitly. A free refunds the owner recorded in the block, whoever ends up
//! freeing it, so dropping a wire needs no charge of its own.

use alloc::boxed::Box;
use alloc::vec::Vec;

/// Maximum payload bytes for one bounded IPC message. Derived from the
/// public wire ABI (`abi::kernel_ipc::IPC_BUF_SIZE`): one queue record never admits
/// more than the documented user-facing buffer.
pub const MAX_IPC_WIRE_PAYLOAD: usize = abi::kernel_ipc::IPC_BUF_SIZE;

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
    /// Exact async operation token, or zero for every legacy send.
    pub async_op: usize,
}

/// Owned kernel buffer representing an in-flight IPC message. The payload is
/// a `Box<[u8]>`: exactly the bounded wire length, no spare capacity.
#[derive(Debug, PartialEq, Eq)]
pub struct IpcWireMessage {
    pub header: IpcWireHeader,
    payload: Box<[u8]>,
}

impl IpcWireMessage {
    /// The incarnation a payload for `header` is charged to.
    fn sender(header: &IpcWireHeader) -> crate::memory::cell_quota::Charge {
        crate::memory::cell_quota::Charge::new(
            types::CellId(header.sender_cell_id),
            header.sender_generation,
        )
    }

    /// Copy `payload` into a wire charged to the sender `header` names.
    /// Fails if payload length exceeds `MAX_IPC_WIRE_PAYLOAD` or allocation fails.
    pub fn try_new(header: IpcWireHeader, payload: &[u8]) -> Result<Self, ()> {
        Self::try_new_charged(header, payload, Self::sender(&header))
    }

    /// Copy `payload` into a wire charged to `charge`, whatever `header` names.
    /// Fails if payload length exceeds `MAX_IPC_WIRE_PAYLOAD` or `charge`
    /// refuses the bytes.
    pub(crate) fn try_new_charged(
        header: IpcWireHeader,
        payload: &[u8],
        charge: crate::memory::cell_quota::Charge,
    ) -> Result<Self, ()> {
        if payload.len() > MAX_IPC_WIRE_PAYLOAD {
            return Err(());
        }
        let payload = Self::copy_payload(payload, charge)?;
        Ok(Self { header, payload })
    }

    /// `bytes` in a buffer of exactly their length charged to `charge`.
    fn copy_payload(
        bytes: &[u8],
        charge: crate::memory::cell_quota::Charge,
    ) -> Result<Box<[u8]>, ()> {
        crate::memory::heap::alloc_for(charge, || {
            let mut vec = Vec::new();
            vec.try_reserve_exact(bytes.len()).map_err(|_| ())?;
            vec.extend_from_slice(bytes);
            Ok(vec.into_boxed_slice())
        })
    }

    /// Construct a wire record, charged to the sender `header` names, directly
    /// from a caller's copy view without a second heap allocation or an
    /// intermediate payload copy.
    pub(crate) fn try_from_user(
        header: IpcWireHeader,
        view: &crate::task::copy_glue::TaskCopyView,
        ptr: usize,
        len: usize,
        #[cfg(feature = "p01-trace")] owner: crate::p01_measurement::Owner,
    ) -> Result<Self, ()> {
        if len > MAX_IPC_WIRE_PAYLOAD {
            return Err(());
        }
        let allocated = crate::memory::heap::alloc_for(Self::sender(&header), || {
            let mut vec = Vec::new();
            vec.try_reserve_exact(len).map_err(|_| ())?;
            vec.resize(len, 0);
            Ok::<_, ()>(vec.into_boxed_slice())
        });
        let mut message = Self {
            header,
            payload: allocated?,
        };
        #[cfg(feature = "p01-trace")]
        message.observe_owner(owner, crate::memory::heap::p01_owner::SENDER_WIRE);
        view.read_into(ptr, message.as_mut_slice())?;
        Ok(message)
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
    /// Kernel-owned reply storage reserved before an async request is accepted.
    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.payload
    }

    #[cfg(feature = "p01-trace")]
    pub(crate) fn observe_synthetic(&self) {
        if self.payload.is_empty() {
            return;
        }
        crate::memory::heap::p01_owner::shared_buffer(
            self.payload.as_ptr() as usize,
            core::alloc::Layout::array::<u8>(self.payload.len()).unwrap(),
            crate::memory::heap::p01_owner::SYNTHETIC_WIRE,
        );
    }
    #[cfg(feature = "p01-trace")]
    pub(crate) fn observe_owner(&self, owner: crate::p01_measurement::Owner, kind: u64) {
        if self.payload.is_empty() {
            return;
        }
        crate::memory::heap::p01_owner::runtime_buffer(
            self.payload.as_ptr() as usize,
            core::alloc::Layout::array::<u8>(self.payload.len()).unwrap(),
            owner,
            kind,
        );
    }
    /// Called only after successful receiver queue commit under SCHEDULER.
    #[cfg(feature = "p01-trace")]
    pub(crate) fn observe_queue_commit(&self, receiver: crate::p01_measurement::Owner) {
        if self.payload.is_empty() {
            return;
        }
        crate::memory::heap::p01_owner::transfer_wire(
            self.payload.as_ptr() as usize,
            core::alloc::Layout::array::<u8>(self.payload.len()).unwrap(),
            receiver,
            self.header.sender_tid as u64,
            self.header.sender_cell_id,
            self.header.sender_generation,
        );
    }

    /// Fallibly duplicate this message into storage charged to `charge`; the
    /// header is copied, never consulted for the charge. Unlike `Clone`,
    /// allocation failure is reported instead of aborting the kernel via the
    /// non-returning OOM path.
    pub(crate) fn try_clone(&self, charge: crate::memory::cell_quota::Charge) -> Result<Self, ()> {
        Ok(Self {
            header: self.header,
            payload: Self::copy_payload(&self.payload, charge)?,
        })
    }
}
