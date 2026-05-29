//! Kernel capability registry.
//!
//! Every open file (or other resource) in ViOS is represented by a `CapId`
//! assigned by this registry.  The registry enforces single-ownership:
//! only the cell that allocated or received a cap may use or revoke it.
//!
//! Current implementation is a simple global `BTreeMap` protected by a
//! Spinlock.  A sharded design (one lock per 256 cap IDs) is deferred
//! to Phase 13 when concurrent pressure is expected to be higher.

use crate::sync::Spinlock;
use alloc::collections::BTreeMap;
use types::{CellId, ViError, ViResult};

/// Capability identifier — re-exported from the API crate so kernel and
/// user-space share exactly one definition.
pub use api::cap::CapId;

/// What resource a capability refers to.
pub enum CapResource {
    /// An open file backed by the kernel-internal FS (`VIFS1` / ramFS).
    ///
    /// The `file` field is `None` while a `ReadCap` syscall is in progress —
    /// the kernel takes the `Box` out, releases the cap-table lock, performs I/O,
    /// then re-acquires the lock and puts the `Box` back.  This prevents the
    /// global cap-table lock from being held across potentially-slow I/O.
    /// A concurrent `ReadCap` on the same cap while it is parked returns
    /// `Err(ViError::TryAgain)`.
    File {
        file: Option<alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>>,
    },
}

/// One capability entry in the table.
pub struct CapEntry {
    /// Cell that currently owns this capability.
    pub owner: CellId,
    /// The resource this capability refers to.
    pub resource: CapResource,
    /// Permissions this cap grants (`api::cap::CapPerms` bits).
    pub perms: u32,
}

/// The global capability table.
pub struct CapTable {
    entries: BTreeMap<CapId, CapEntry>,
    next_id: u64,
}

impl CapTable {
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            next_id: 1, // 0 is the null sentinel — never assigned
        }
    }

    /// Allocate a new capability for `owner` over `resource`.
    ///
    /// Returns the new `CapId` (always > 0 for the first 2^64-1 calls).
    /// Panics in debug builds if `next_id` wraps to 0 (effectively unreachable).
    pub fn alloc(&mut self, owner: CellId, resource: CapResource, perms: u32) -> CapId {
        let id = CapId(self.next_id);
        let next = self.next_id.wrapping_add(1);
        debug_assert!(next != 0, "CapId counter wrapped to 0 — unreachable in practice");
        self.next_id = if next == 0 { 1 } else { next };
        self.entries.insert(id, CapEntry { owner, resource, perms });
        id
    }

    /// Verify that `caller` owns `cap_id`.
    ///
    /// # Errors
    /// Returns `ViError::PermissionDenied` if the cap is unknown or owned by
    /// a different cell.
    pub fn verify(&self, cap_id: CapId, caller: CellId) -> ViResult<()> {
        match self.entries.get(&cap_id) {
            Some(e) if e.owner == caller => Ok(()),
            Some(_) => Err(ViError::PermissionDenied),
            None => Err(ViError::NotFound),
        }
    }

    /// Revoke a capability.  Idempotent — no-op if the cap is not in the table.
    pub fn revoke(&mut self, cap_id: CapId) {
        self.entries.remove(&cap_id);
    }

    /// Revoke all capabilities owned by `owner`.
    ///
    /// Called when a cell exits to prevent orphaned caps.
    pub fn revoke_all_for(&mut self, owner: CellId) {
        self.entries.retain(|_, e| e.owner != owner);
    }

    /// Return an immutable reference to an entry if owned by `caller`.
    pub fn get_if_owner(&self, cap_id: CapId, caller: CellId) -> Option<&CapEntry> {
        self.entries.get(&cap_id).filter(|e| e.owner == caller)
    }

    /// Return a mutable reference to an entry (does NOT verify ownership —
    /// callers must call `verify` first).
    pub fn get_mut_unchecked(&mut self, cap_id: CapId) -> Option<&mut CapEntry> {
        self.entries.get_mut(&cap_id)
    }

    /// Take the `Box<dyn ViFile>` out of a `CapResource::File` entry so that
    /// I/O can be performed with the cap-table lock released.
    ///
    /// Returns `Err` if the cap is not found, already parked (concurrent read),
    /// or is not a `File` cap.
    pub fn park_file(
        &mut self,
        cap_id: CapId,
        owner: types::CellId,
    ) -> ViResult<alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>> {
        self.verify(cap_id, owner)?;
        let entry = self.entries.get_mut(&cap_id).ok_or(ViError::NotFound)?;
        // Single variant today; match for future exhaustiveness when more cap types land.
        let CapResource::File { ref mut file } = entry.resource;
        // None means a concurrent read is in progress (single-core: shouldn't happen).
        file.take().ok_or(ViError::InvalidInput)
    }

    /// Return a previously parked `Box<dyn ViFile>` back into the cap entry.
    ///
    /// No-ops if the cap was revoked while the file was parked.
    pub fn unpark_file(
        &mut self,
        cap_id: CapId,
        boxed_file: alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>,
    ) {
        if let Some(CapEntry { resource: CapResource::File { ref mut file }, .. }) =
            self.entries.get_mut(&cap_id)
        {
            *file = Some(boxed_file);
        }
        // If the cap was revoked while parked, just drop `boxed_file` here.
    }
}

/// Global kernel capability table.
pub static CAP_TABLE: Spinlock<CapTable> = Spinlock::new(CapTable::new());
