//! Renames within one mount.
//!
//! Two contracts, chosen by `dir_ops` from where the names lie: `/srv`
//! publishes without replacing (CellosFS, atomic), `/mnt/disk` follows POSIX
//! `rename` and replaces (FAT, `backend_fat::rename`).
//!
//! Open handles keep the `UnlinkAt`/`RmdirAt` contract: a handle names a path,
//! not an object, and is never moved or revoked by a rename. Every file
//! handle at or below either name is marked modified and every directory
//! handle at or below either name, or on either parent, is marked changed —
//! whether or not the backend reported success. A handle on the old name then
//! names nothing, or whatever is later created there; one on the new name
//! names the moved object. Neither happens silently: the sticky marks report
//! it (`FileHandleState.modified`, `DirPage` `CHANGED`), and a holder that
//! wants the moved object reopens it by its new name.

use crate::backend::FsBackend;
use crate::fs_error::FsError;
use crate::manager::VfsManager;

impl VfsManager {
    /// Atomic no-replace rename (the `/srv` contract).
    pub fn rename_no_replace(&mut self, old: &str, new: &str) -> Result<(), FsError> {
        self.rename_within_mount(old, new, |backend, old, new| {
            backend.rename_no_replace(old, new)
        })
    }

    /// POSIX rename replacing a file or an empty directory at `new` (the
    /// `/mnt/disk` contract).
    pub fn rename_replace(&mut self, old: &str, new: &str) -> Result<(), FsError> {
        self.rename_within_mount(old, new, |backend, old, new| {
            backend.rename_replace(old, new)
        })
    }

    /// `Unsupported` across two backends: no backend renames between mounts.
    fn rename_within_mount(
        &mut self,
        old: &str,
        new: &str,
        rename: impl FnOnce(&mut dyn FsBackend, &str, &str) -> Result<(), FsError>,
    ) -> Result<(), FsError> {
        let (Some(old_backend), Some(new_backend)) =
            (self.mounts.backend(old), self.mounts.backend(new))
        else {
            return Err(FsError::Unavailable);
        };
        if !core::ptr::eq(old_backend, new_backend) {
            return Err(FsError::Unsupported);
        }
        if !self.healthy(old) || !self.healthy(new) {
            return Err(FsError::Unavailable);
        }
        let result = self
            .mounts
            .backend_mut(old)
            .map_or(Err(FsError::Unavailable), |backend| {
                rename(backend, old, new)
            });
        self.mark_tree(old);
        self.mark_tree(new);
        result
    }
}
