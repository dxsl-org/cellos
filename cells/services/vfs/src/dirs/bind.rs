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
//! Legacy spawn admission binds at first contact. PreparedChildV1 instead
//! reserves unpublished entries here and installs their seal before publication
//! can be reconciled at the first request; it never uses legacy live-child bind.
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
            match self.entries.get(&raw).filter(|e| e.owner == spawner && e.published) {
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
                // ID exhaustion can occur after an earlier entry was issued.
                // Roll the whole set back; consumed IDs are never reused.
                Err(_) => {
                    let _ = self.revoke_ids(&issued);
                    return BindOutcome::Refused;
                }
            }
        }
        BindOutcome::Bound(issued.len())
    }

    /// Reserve a fresh handle in the existing lineage table without making it
    /// usable. The child namespace is sealed before even a fast-path lookup.
    ///
    /// Directory ids revoked by replacing an older generation's state are appended to
    /// `revoked` on every outcome, so the caller can revoke the file handles opened
    /// through them even when the reservation itself is refused.
    pub(crate) fn reserve_prepared(
        &mut self,
        parent: Caller,
        child: Caller,
        source: api::dir_handles::ViDirHandle,
        revoked: &mut Vec<u64>,
    ) -> Result<api::dir_handles::ViDirHandle, super::DirError> {
        let path = self.owned(parent, source)
            .ok_or(super::DirError::UnknownHandle)?.path.clone();
        if self.state(child).is_some() {
            return Err(super::DirError::UnknownHandle);
        }
        if let super::lifecycle::Contact::NeedsAttestation { revoked_dir_ids, .. } = self.on_contact(child) {
            revoked.extend(revoked_dir_ids);
        }
        self.seal(child);
        self.mark_attested(child);
        match self.insert(child, path, Some(source.0)) {
            Ok(handle) => {
                self.entries.get_mut(&handle.0).expect("just reserved").published = false;
                Ok(handle)
            }
            Err(error) => {
                // Remove only the state this call created. A newer generation of the
                // same cell id keeps its seal, handles and handle count.
                if self.state(child).is_some() {
                    self.cells.remove(&child.cell.0);
                }
                Err(error)
            }
        }
    }

    /// No validation after publication: every entry and the seal were reserved
    /// together. A missing lineage is retirement, never an ambient cwd fallback.
    pub(crate) fn publish_prepared(&mut self, child: Caller, handle: u64) -> bool {
        let Some(entry) = self.entries.get_mut(&handle).filter(|e| e.owner == child) else {
            return false;
        };
        entry.published = true;
        true
    }
}
