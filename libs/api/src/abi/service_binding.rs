// SPDX-License-Identifier: Apache-2.0
//! Kernel-attested local service-provider binding ABI (`LookupServiceBound`).
//!
//! `LookupService` answers with a bare TID, and a TID is not a provider identity:
//! it says where to send, not *which* Cell incarnation is there. This record adds
//! the identity the TID cannot carry, using the kernel's existing per-Cell epoch
//! rather than a second token type:
//!
//! - `tid` — the routable task id. Never re-issued within one boot, so it is safe
//!   to carry; it is not the identity, because it is a transport detail.
//! - `cell_id` — the owning Cell. Slots **are** reused, so this alone is not
//!   identity either.
//! - `generation` — the per-Cell epoch the kernel mints on cell creation. This is
//!   the identity, and it is the same axis [`crate::caller_identity::CallerIdentity`]
//!   and [`crate::cell_owner::CellOwner`] already use.
//!
//! The kernel writes the record in one instant under the scheduler lock, so the
//! three fields cannot disagree. A caller holds it as a *descriptor*: it is not a
//! capability, it grants nothing, and the kernel re-verifies the live binding when
//! the resolved TID is actually used.
//!
//! Absent, short, or non-live bytes mean "no binding", and the only correct
//! response to that is to refuse the call rather than to send to a stale TID.

/// Exact byte size written by `LookupServiceBound`.
pub const SERVICE_BINDING_LEN: usize = 24;

/// One live local service provider, as the kernel states it.
///
/// Construct only from kernel-written bytes ([`ServiceBinding::from_bytes`]) or,
/// inside the kernel, from live scheduler state — never from a peer's or a
/// caller's self-report.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceBinding {
    /// Current provider task id. A transport detail, never an identity.
    pub tid: u64,
    /// Owning Cell of the provider. A reusable slot, never an identity alone.
    pub cell_id: u64,
    /// Per-Cell epoch minted at cell creation. This is the identity.
    pub generation: u64,
}

impl ServiceBinding {
    pub const fn new(tid: u64, cell_id: u64, generation: u64) -> Self {
        Self {
            tid,
            cell_id,
            generation,
        }
    }

    /// Whether all three fields name a live provider.
    ///
    /// `tid == 0` is the ABI "no provider" sentinel, and `cell_id == 0` is the
    /// kernel's own allocation rather than any Cell — neither can name a service.
    pub const fn is_live(&self) -> bool {
        self.tid != 0 && self.cell_id != 0 && self.generation != 0
    }

    pub fn to_bytes(self) -> [u8; SERVICE_BINDING_LEN] {
        let mut out = [0; SERVICE_BINDING_LEN];
        out[0..8].copy_from_slice(&self.tid.to_le_bytes());
        out[8..16].copy_from_slice(&self.cell_id.to_le_bytes());
        out[16..24].copy_from_slice(&self.generation.to_le_bytes());
        out
    }

    /// Parse a kernel-written record.
    ///
    /// Returns `None` for a short slice or a record that names no live provider.
    /// Both are "no binding", and a caller must treat that as a refusal.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < SERVICE_BINDING_LEN {
            return None;
        }
        let word = |offset: usize| -> Option<u64> {
            Some(u64::from_le_bytes(
                bytes[offset..offset + 8].try_into().ok()?,
            ))
        };
        let binding = Self {
            tid: word(0)?,
            cell_id: word(8)?,
            generation: word(16)?,
        };
        binding.is_live().then_some(binding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_is_fixed_and_rejects_invalid_values() {
        let binding = ServiceBinding::new(9, 3, 41);
        assert_eq!(core::mem::size_of::<ServiceBinding>(), SERVICE_BINDING_LEN);
        assert_eq!(ServiceBinding::from_bytes(&binding.to_bytes()), Some(binding));
        assert!(ServiceBinding::from_bytes(&[0; SERVICE_BINDING_LEN]).is_none());
    }

    #[test]
    fn short_slice_is_never_a_partial_binding() {
        let binding = ServiceBinding::new(9, 3, 41);
        let bytes = binding.to_bytes();
        assert!(ServiceBinding::from_bytes(&bytes[..SERVICE_BINDING_LEN - 1]).is_none());
        assert!(ServiceBinding::from_bytes(&[]).is_none());
    }

    #[test]
    fn every_zero_field_denies_the_binding() {
        // A generation-less Cell cannot name a service: the epoch is the identity.
        assert!(ServiceBinding::from_bytes(&ServiceBinding::new(9, 3, 0).to_bytes()).is_none());
        // cell_id 0 is the kernel's own allocation, not a Cell.
        assert!(ServiceBinding::from_bytes(&ServiceBinding::new(9, 0, 41).to_bytes()).is_none());
        // tid 0 is the ABI "no provider" sentinel.
        assert!(ServiceBinding::from_bytes(&ServiceBinding::new(0, 3, 41).to_bytes()).is_none());
    }
}
