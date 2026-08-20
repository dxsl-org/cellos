//! Binding the handle set a cell's spawner named at spawn.
//!
//! The kernel copies a set of opaque values from a spawner to the cell it
//! spawns and will state, on its own authority, which cell supplied them. That
//! is provenance, not authority: what the spawner *named* and what the spawner
//! *holds* are different sets, because a cell acquires handles from this service
//! after it starts, and only this service knows the second one.
//!
//! So the check lives here, and it is all-or-nothing. If any named handle was
//! not held by the attested spawner, none are bound. Binding the valid subset
//! would hand the child a narrower authority than either side asked for, with
//! nothing failing and nobody told — the quiet downgrade that makes a capability
//! system unauditable.
//!
//! ## The consequence, stated rather than glossed
//! An over-broad spawn does not fail the spawn. The child exists and its first
//! filesystem call is refused. That is still fail-closed — the child holds no
//! filesystem authority, not extra authority — but the failure surfaces later
//! than its cause. Closing the gap would need the kernel to consult this service
//! from inside a spawn syscall, which is a layering inversion and a deadlock
//! hazard.
//!
//! ## Why a cell with an inherited set is sealed
//! A spawner that hands a child directory handles has placed it in the
//! capability world; leaving path strings open to it would make the grant
//! decorative. Sealing happens whether or not the bind succeeded, and that
//! direction is deliberate: if a refused bind left path strings available, the
//! failure would *widen* the child's reach relative to success.

use alloc::string::String;
use alloc::vec::Vec;
use api::dir_attestation::ViDirHandleAttestation;
use types::CellId;

use super::DirTable;
use crate::caller::Caller;

/// What came of a cell's attested set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindOutcome {
    /// The kernel names no inherited set for this cell.
    NothingNamed,
    /// The record describes a different cell than the one that called.
    NotThisCell,
    /// Every named handle checked out. This many were bound to the child.
    Bound(usize),
    /// At least one named handle was not held by the attested spawner, or the
    /// child cannot hold them all. Nothing was bound.
    Refused,
}

impl DirTable {
    /// Bind what `child`'s spawner named, if the spawner genuinely held it all.
    ///
    /// Each bound entry is a new handle owned by the child and recorded as
    /// derived from the spawner's, so revoking the spawner's handle revokes the
    /// child's with it.
    pub fn bind_inherited(
        &mut self,
        child: Caller,
        record: &ViDirHandleAttestation,
    ) -> BindOutcome {
        if record.cell_id != child.cell.0 || record.generation != child.generation {
            return BindOutcome::NotThisCell;
        }
        let named = record.set.as_slice();
        if named.is_empty() {
            return BindOutcome::NothingNamed;
        }
        let spawner = Caller::principal(CellId(record.spawner_cell_id), record.spawner_generation);

        // Resolve every named handle against the spawner's own entries first.
        // Nothing is inserted until the whole set has checked out.
        let mut resolved: Vec<(u64, String)> = Vec::with_capacity(named.len());
        for &raw in named {
            match self.entries.get(&raw).filter(|e| e.owner == spawner) {
                Some(entry) => resolved.push((raw, entry.path.clone())),
                None => return BindOutcome::Refused,
            }
        }
        if self.held_by(child) + resolved.len() > super::MAX_HANDLES_PER_CELL {
            return BindOutcome::Refused;
        }

        let mut issued: Vec<u64> = Vec::with_capacity(resolved.len());
        for (parent, path) in resolved {
            match self.insert(child, path, Some(parent)) {
                Ok(handle) => issued.push(handle.0),
                // Unreachable given the limit check above, but unwound anyway:
                // a partial bind is the one outcome this function exists to
                // prevent, and "cannot happen" is not a reason to leave the
                // half-bound state reachable.
                Err(_) => {
                    let _ = self.revoke_ids(&issued);
                    return BindOutcome::Refused;
                }
            }
        }
        BindOutcome::Bound(issued.len())
    }
}
