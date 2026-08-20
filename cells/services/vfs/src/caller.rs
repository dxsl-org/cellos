//! Who VFS is serving, according to the kernel.
//!
//! VFS used to build a `CellId` out of the sender tid. That was wrong twice: a
//! thread reports its own tid while belonging to its parent cell, so its quota
//! and its handles landed under a `CellId` matching no cell at all; and it made
//! identity a property of the transport rather than something vouched for. Both
//! disappear once the kernel states the identity — see `api::caller_identity`.
//!
//! Every authorization decision and every quota charge in this cell reads a
//! [`Caller`], and a [`Caller`] can only be built from an attested identity.

use api::caller_identity::CallerIdentity;
use types::CellId;

/// A caller VFS is willing to act for.
///
/// Two callers are the same principal only when both the cell and its generation
/// match: a respawned cell that reused a dead cell's `CellId` is a *different*
/// principal, and must not inherit the dead cell's handles, pending reads, or
/// quota credit.
#[derive(Debug, Clone, Copy)]
pub struct Caller {
    /// Owning cell of the calling task — the cell, never the thread.
    pub cell: CellId,
    /// Cell epoch from the attestation. Distinguishes a successor cell from the
    /// one it replaced.
    pub generation: u64,
    /// Sending thread the kernel attested for this request.
    pub sender_tid: u64,
}

impl PartialEq for Caller {
    fn eq(&self, other: &Self) -> bool {
        self.cell == other.cell && self.generation == other.generation
    }
}

impl Eq for Caller {}

impl Caller {
    /// Build a cell-generation principal when no current sender thread is part
    /// of the authority being modeled.
    pub const fn principal(cell: CellId, generation: u64) -> Self {
        Self {
            cell,
            generation,
            sender_tid: 0,
        }
    }

    /// Adopt a kernel-attested identity.
    ///
    /// The only constructor outside tests, which is the point: there is no way to
    /// obtain a `Caller` from request bytes.
    pub fn from_attested(id: CallerIdentity) -> Self {
        Self {
            cell: CellId(id.cell_id),
            generation: id.generation,
            sender_tid: id.sender_tid,
        }
    }

    /// Whether durable per-caller state (open handles, pending reads) may be
    /// recorded against this caller.
    ///
    /// Requires an attested generation: without one, a later caller under the
    /// same `CellId` would be indistinguishable from this one, which is exactly
    /// the confusion the generation exists to prevent. Paths that only consult
    /// path-keyed policy do not need this.
    pub fn may_own_state(&self) -> bool {
        self.cell.0 != 0 && self.generation != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attested_identity_becomes_the_cell_not_the_sending_thread() {
        let caller = Caller::from_attested(CallerIdentity {
            cell_id: 4,
            generation: 9,
            sender_tid: 77, // a thread of cell 4
        });
        assert_eq!(caller.cell, CellId(4));
        assert!(caller.may_own_state());
    }

    #[test]
    fn same_cell_different_generation_is_a_different_principal() {
        let old = Caller {
            cell: CellId(4),
            generation: 1,
            sender_tid: 70,
        };
        let respawned = Caller {
            cell: CellId(4),
            generation: 2,
            sender_tid: 71,
        };
        assert_ne!(old, respawned);
    }

    #[test]
    fn same_cell_generation_different_threads_is_the_same_principal() {
        let left = Caller {
            cell: CellId(4),
            generation: 2,
            sender_tid: 70,
        };
        let right = Caller {
            cell: CellId(4),
            generation: 2,
            sender_tid: 71,
        };
        assert_eq!(left, right);
        assert!(left.may_own_state());
    }

    #[test]
    fn unattested_generation_may_not_own_state() {
        let caller = Caller {
            cell: CellId(4),
            generation: 0,
            sender_tid: 77,
        };
        assert!(!caller.may_own_state());
    }
}
