//! `FsBackend` — the contract every mounted filesystem implements.
//!
//! Contract: all methods receive the ABSOLUTE VFS path (e.g. `/data/log.txt`),
//! never a pre-stripped relative path. Each backend owns the stripping of its
//! mount prefix because rejection semantics are backend-specific (FAT rejects
//! empty-rel writes and `..` components; RamFS resolves the whole tree). This
//! mirrors the pre-MountTable dispatch exactly — do not centralize stripping
//! without re-validating every path edge case.
//!
//! Mutability: read ops take `&self`; mutating ops take `&mut self` because
//! RamFS mutates its tree (FAT uses fatfs interior mutability and ignores it).

use alloc::vec::Vec;
use wire::ipc::vfs_dir_page::{push_entry, EntryKind};

use crate::fs_error::FsError;

/// Cursors a backend hands out stay below this bound. The bits above it belong
/// to the layers that union several sources into one listing:
/// `BinOverlay` uses bit 30 and `VfsManager` bit 31.
pub(crate) const BACKEND_CURSOR_LIMIT: u32 = 1 << 30;

/// One page written by [`FsBackend::list_page`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListPage {
    /// Bytes of packed `wire::ipc::vfs_dir_page` entries written to `out`.
    pub written: usize,
    /// Cursor of the first entry that did not fit; `None` when this page ends
    /// the listing.
    pub next: Option<u32>,
}

/// Page an ordered entry sequence: skip `start` entries, then pack whole
/// entries into `out` until one does not fit.
///
/// An error (never a short page) when iteration fails (the entry's own error),
/// a name is not a valid directory component or `start` lies past the end (a
/// stale cursor) — both `InvalidPath` — or the sequence outgrows
/// [`BACKEND_CURSOR_LIMIT`] (`Unsupported`).
pub(crate) fn fill_page<N: AsRef<[u8]>>(
    entries: impl Iterator<Item = Result<(EntryKind, N), FsError>>,
    start: u32,
    out: &mut [u8],
) -> Result<ListPage, FsError> {
    let mut index = 0u32;
    let mut written = 0;
    for entry in entries {
        let (kind, name) = entry?;
        if index >= start {
            match push_entry(out, written, kind, name.as_ref()) {
                Ok(Some(end)) => written = end,
                Ok(None) => {
                    return Ok(ListPage {
                        written,
                        next: Some(index),
                    })
                }
                Err(_) => return Err(FsError::InvalidPath),
            }
        }
        index += 1;
        if index >= BACKEND_CURSOR_LIMIT {
            return Err(FsError::Unsupported);
        }
    }
    if index < start {
        return Err(FsError::InvalidPath);
    }
    Ok(ListPage {
        written,
        next: None,
    })
}

/// Operational state only: healthy does not certify a durability barrier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendHealth {
    Ready,
    Unavailable,
    Poisoned,
}

/// `Send` bound (Law 7): backends live inside the `GLOBAL_VFS` static Mutex,
/// which requires its contents to be `Send` for the static to be `Sync`.
pub trait FsBackend: Send {
    fn health(&self) -> BackendHealth {
        BackendHealth::Ready
    }
    /// SAS zero-copy pointer to the file's bytes. Only in-memory backends
    /// return `Some` — pointers must stay valid while the VFS cell lives.
    /// Disk backends return `None`; callers fall back to the copy path.
    fn get_file_ptr(&self, path: &str) -> Option<(usize, usize)>;

    /// List directory entries as `d:name\n` / `f:name\n` lines into `out`.
    /// Returns bytes written (0 for an empty directory); an error when the
    /// path is missing, not a directory, or the listing could not be read.
    fn list(&self, path: &str, out: &mut [u8]) -> Result<usize, FsError>;

    /// Page the entries of directory `path` into `out`, starting at entry
    /// index `start` (see [`fill_page`]). Entry order is the backend's own and
    /// must be stable while the directory is unchanged.
    ///
    /// An error when `path` is missing or not a directory, iteration failed, a
    /// name is not a valid component, or `start` is stale. Never a silent stop.
    fn list_page(&self, path: &str, start: u32, out: &mut [u8]) -> Result<ListPage, FsError>;

    /// `Ok(Some((size, is_dir)))`, `Ok(None)` when the path does not exist, an
    /// error when existence could not be determined.
    fn stat(&self, path: &str) -> Result<Option<(u64, bool)>, FsError>;

    /// File size for quota accounting. `NotFound` when absent (callers charge
    /// net delta and treat that as 0).
    fn file_size(&self, path: &str) -> Result<u64, FsError>;

    /// Whole-file copy for the async-read handle table. A device failure part
    /// way through is an error, never the bytes read so far.
    fn read_to_vec(&self, path: &str) -> Result<Vec<u8>, FsError>;

    /// Create-or-truncate write. Authorization is the dispatcher's job
    /// (AccessTable); backends may still enforce structural rules.
    fn write(&mut self, path: &str, content: &[u8]) -> Result<(), FsError>;
    /// Positional bounded read: bytes read, 0 at or past end of file.
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let _ = (path, offset, buf);
        Err(FsError::Unsupported)
    }

    /// Positional bounded write.
    fn write_at(&mut self, path: &str, offset: u64, content: &[u8]) -> Result<(), FsError> {
        let _ = (path, offset, content);
        Err(FsError::Unsupported)
    }

    /// Flush dirty pages for a path to the block device.
    fn sync(&mut self, path: &str) -> Result<(), FsError> {
        let _ = path;
        Err(FsError::Unsupported)
    }

    fn append(&mut self, path: &str, content: &[u8]) -> Result<(), FsError>;
    /// mkdir -p semantics on FAT; single-level create on RamFS.
    fn mkdir(&mut self, path: &str) -> Result<(), FsError>;

    /// Remove an EMPTY directory (POSIX ENOTDIR/ENOTEMPTY semantics).
    fn rmdir(&mut self, path: &str) -> Result<(), FsError>;

    /// Remove a regular FILE (directories are rejected).
    fn unlink(&mut self, path: &str) -> Result<(), FsError>;

    /// Recursive delete (`rm -r`).
    fn rmdir_recursive(&mut self, path: &str) -> Result<(), FsError>;

    /// Atomic no-replace rename from `old` to `new`.
    ///
    /// Only the `/srv` backend (CellosFS Native) implements it.
    fn rename_no_replace(&mut self, old: &str, new: &str) -> Result<(), FsError> {
        let _ = (old, new);
        Err(FsError::Unsupported)
    }

    // ── D03 conditional save. Defaults: unsupported (FAT32, bootfs, RamFS). ──

    /// Staging object path for transaction `id`, inside this backend; `None`
    /// when the backend has no conditional publication.
    fn stage_path(&self, id: u32) -> Option<alloc::string::String> {
        let _ = id;
        None
    }

    /// Exact identity of `path` (absence is `Observation::Absent`); `Ok(None)`
    /// when the backend can name no identity for it.
    fn observe(&self, path: &str) -> Result<Option<wire::ipc::Observation>, FsError> {
        let _ = path;
        Err(FsError::Unsupported)
    }

    /// Create the empty staging object exclusively. Not synced: staged bytes
    /// become persistent only with the commit's sync.
    fn stage_create(&mut self, stage: &str) -> Result<(), FsError> {
        let _ = stage;
        Err(FsError::Unsupported)
    }

    /// Positional write into a staging object, not synced.
    fn stage_write(&mut self, stage: &str, offset: u64, bytes: &[u8]) -> Result<(), FsError> {
        let _ = (stage, offset, bytes);
        Err(FsError::Unsupported)
    }

    /// Atomically check that `stage` is still `sealed` and `dst` still matches
    /// `expect`, then publish `stage` at `dst` and sync.
    fn publish_if(
        &mut self,
        stage: &str,
        sealed: (u64, u64),
        dst: &str,
        expect: wire::ipc::SaveExpect,
    ) -> wire::ipc::SaveOutcome {
        let _ = (stage, sealed, dst, expect);
        wire::ipc::SaveOutcome::NotPublished(wire::ipc::save_refusal::REFUSED)
    }
}
