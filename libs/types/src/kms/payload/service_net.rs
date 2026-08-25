use super::{put_u64, read_u64};
use crate::kms::ServiceNetBindingEpoch;

/// Response to `RegisterServiceNetInstance`.
///
/// The bound cell, generation, and live service TID are independent from the
/// broker binding and must all match on every relay operation.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceNetBindingPayload {
    pub binding_epoch: ServiceNetBindingEpoch,
    pub bound_cell_id: u64,
    pub bound_generation: u64,
    pub bound_service_tid: u64,
}

impl ServiceNetBindingPayload {
    pub const LEN: usize = 32;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.binding_epoch.0);
        put_u64(&mut out, 8, self.bound_cell_id);
        put_u64(&mut out, 16, self.bound_generation);
        put_u64(&mut out, 24, self.bound_service_tid);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        (bytes.len() == Self::LEN).then(|| Self {
            binding_epoch: ServiceNetBindingEpoch(read_u64(bytes, 0)),
            bound_cell_id: read_u64(bytes, 8),
            bound_generation: read_u64(bytes, 16),
            bound_service_tid: read_u64(bytes, 24),
        })
    }
}

const _: () = assert!(core::mem::size_of::<ServiceNetBindingPayload>() == 32);
