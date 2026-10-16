//! Kernel capability registry.
//!
//! Every open file (or other resource) in ViCell is represented by a `CapId`
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
    /// Optional lease expiry in kernel monotonic ticks.
    ///
    /// `None` = permanent (revoked only by explicit `close` or cell exit).
    /// `Some(t)` = auto-revoked on the first `verify` call after tick `t`.
    pub expires_at: Option<u64>,
}

/// The global capability table.
pub struct CapTable {
    entries: BTreeMap<CapId, CapEntry>,
    next_id: u64,
}

impl Default for CapTable {
    fn default() -> Self {
        Self::new()
    }
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
    /// Refuses exhausted ID space without changing existing entries. IDs are
    /// never wrapped or reused, including after a capability is revoked.
    pub fn alloc(&mut self, owner: CellId, resource: CapResource, perms: u32) -> ViResult<CapId> {
        let next = self.next_id.checked_add(1).ok_or(ViError::OutOfMemory)?;
        let id = CapId(self.next_id);
        self.next_id = next;
        #[cfg(cellos_p01_trace)]
        let _semantic = crate::memory::heap::p01_owner::creation(
            None, crate::memory::heap::p01_owner::SERVICE_SHARED);
        self.entries.insert(
            id,
            CapEntry {
                owner,
                resource,
                perms,
                expires_at: None,
            },
        );
        Ok(id)
    }

    /// Allocate a capability with an automatic lease expiry.
    ///
    /// `expires_at` is the absolute kernel tick at which the cap becomes invalid.
    /// Use `crate::task::system_ticks() + duration` to compute this.
    pub fn alloc_with_lease(
        &mut self,
        owner: CellId,
        resource: CapResource,
        perms: u32,
        expires_at: u64,
    ) -> ViResult<CapId> {
        let id = self.alloc(owner, resource, perms)?;
        if let Some(e) = self.entries.get_mut(&id) {
            e.expires_at = Some(expires_at);
        }
        Ok(id)
    }

    /// Verify that `caller` owns `cap_id` and the lease has not expired.
    ///
    /// Lease expiry is checked lazily — the entry is revoked and
    /// `ViError::PermissionDenied` returned if `expires_at` has passed.
    ///
    /// # Errors
    /// - `ViError::PermissionDenied` — cap owned by a different cell or lease expired.
    /// - `ViError::NotFound` — cap does not exist.
    pub fn verify(&mut self, cap_id: CapId, caller: CellId) -> ViResult<()> {
        let now = crate::task::system_ticks() as u64;
        match self.entries.get(&cap_id) {
            Some(e) if e.owner == caller => {
                // Lazy lease check.
                if let Some(exp) = e.expires_at {
                    if now >= exp {
                        // Expired — revoke and report.
                        self.entries.remove(&cap_id);
                        return Err(ViError::PermissionDenied);
                    }
                }
                Ok(())
            }
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
        required_perm: api::cap::CapPerms,
    ) -> ViResult<alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>> {
        self.verify(cap_id, owner)?;
        let entry = self.entries.get_mut(&cap_id).ok_or(ViError::NotFound)?;
        if !api::cap::CapPerms(entry.perms).has(required_perm) {
            return Err(ViError::PermissionDenied);
        }
        let CapResource::File { ref mut file } = entry.resource;
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
        if let Some(CapEntry {
            resource: CapResource::File { ref mut file },
            ..
        }) = self.entries.get_mut(&cap_id)
        {
            *file = Some(boxed_file);
        }
        // If the cap was revoked while parked, just drop `boxed_file` here.
    }
}

/// Global kernel capability table.
pub static CAP_TABLE: Spinlock<CapTable> = Spinlock::new(CapTable::new());

/// Exact backing storage for CSpace-managed BootFS files indexed by (cell_id, generation, ticket).
static CSPACE_FILES: Spinlock<BTreeMap<(u64, u64, u64), Option<alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>>>> =
    Spinlock::new(BTreeMap::new());

pub fn attach_file_to_ticket(
    key: (u64, u64, u64),
    file: alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>,
) {
    CSPACE_FILES.lock().insert(key, Some(file));
}

pub fn park_ticket_file(
    key: (u64, u64, u64),
) -> Option<alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>> {
    CSPACE_FILES.lock().get_mut(&key).and_then(|opt| opt.take())
}

pub fn unpark_ticket_file(
    key: (u64, u64, u64),
    file: alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>,
) {
    if let Some(opt) = CSPACE_FILES.lock().get_mut(&key) {
        *opt = Some(file);
    }
}

pub fn remove_ticket_file(key: (u64, u64, u64)) {
    CSPACE_FILES.lock().remove(&key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausted_ids_preserve_foreign_owners_and_never_reissue_after_close() {
        let mut table = CapTable::new();
        let owner = CellId(1);
        let peer = CellId(2);
        let original = table.alloc(owner, CapResource::File { file: None }, 1).unwrap();
        table.next_id = u64::MAX - 1;
        let last = table.alloc(peer, CapResource::File { file: None }, 2).unwrap();
        assert_eq!(last.0, u64::MAX - 1);
        for _ in 0..2 {
            assert_eq!(table.alloc(peer, CapResource::File { file: None }, 4),
                Err(ViError::OutOfMemory));
            assert_eq!(table.get_if_owner(original, owner).unwrap().perms, 1);
            assert!(table.get_if_owner(original, peer).is_none());
            assert_eq!(table.get_if_owner(last, peer).unwrap().perms, 2);
            assert_eq!(table.entries.len(), 2);
            assert_eq!(table.next_id, u64::MAX);
        }
        table.revoke(original);
        table.revoke(last);
        assert_eq!(table.alloc(owner, CapResource::File { file: None }, 1),
            Err(ViError::OutOfMemory));
        assert!(table.entries.is_empty());
    }

    #[test]
    fn failed_leased_allocation_cannot_change_a_live_entry_or_lease() {
        let mut table = CapTable::new();
        let owner = CellId(3);
        table.next_id = u64::MAX - 1;
        let live = table.alloc_with_lease(owner, CapResource::File { file: None }, 1, 100).unwrap();
        assert_eq!(table.alloc_with_lease(CellId(4), CapResource::File { file: None }, 7, 999),
            Err(ViError::OutOfMemory));
        let entry = table.get_if_owner(live, owner).unwrap();
        assert_eq!((entry.perms, entry.expires_at), (1, Some(100)));
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.next_id, u64::MAX);
    }
}
