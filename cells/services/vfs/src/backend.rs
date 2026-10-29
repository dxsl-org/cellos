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
use api::ipc::vfs_dir_page::{push_entry, EntryKind};

/// Cursors a backend hands out stay below this bound. The bits above it belong
/// to the layers that union several sources into one listing:
/// `BinOverlay` uses bit 30 and `VfsManager` bit 31.
pub(crate) const BACKEND_CURSOR_LIMIT: u32 = 1 << 30;

/// One page written by [`FsBackend::list_page`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListPage {
    /// Bytes of packed `api::ipc::vfs_dir_page` entries written to `out`.
    pub written: usize,
    /// Cursor of the first entry that did not fit; `None` when this page ends
    /// the listing.
    pub next: Option<u32>,
}

/// Page an ordered entry sequence: skip `start` entries, then pack whole
/// entries into `out` until one does not fit.
///
/// `None` (never a short page) when iteration fails, a name is not a valid
/// directory component, `start` lies past the end (a stale cursor), or the
/// sequence outgrows [`BACKEND_CURSOR_LIMIT`].
pub(crate) fn fill_page<N: AsRef<[u8]>>(
    entries: impl Iterator<Item = Result<(EntryKind, N), ()>>,
    start: u32,
    out: &mut [u8],
) -> Option<ListPage> {
    let mut index = 0u32;
    let mut written = 0;
    for entry in entries {
        let (kind, name) = entry.ok()?;
        if index >= start {
            match push_entry(out, written, kind, name.as_ref()) {
                Ok(Some(end)) => written = end,
                Ok(None) => return Some(ListPage { written, next: Some(index) }),
                Err(_) => return None,
            }
        }
        index += 1;
        if index >= BACKEND_CURSOR_LIMIT {
            return None;
        }
    }
    (index >= start).then_some(ListPage { written, next: None })
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
    /// Returns bytes written; 0 when the path is missing or not a directory.
    fn list(&self, path: &str, out: &mut [u8]) -> usize;

    /// Page the entries of directory `path` into `out`, starting at entry
    /// index `start` (see [`fill_page`]). Entry order is the backend's own and
    /// must be stable while the directory is unchanged.
    ///
    /// `None` when `path` is missing or not a directory, iteration failed, a
    /// name is not a valid component, or `start` is stale. Never a silent stop.
    fn list_page(&self, path: &str, start: u32, out: &mut [u8]) -> Option<ListPage>;

    /// `(size, is_dir)`, or `None` when the path does not exist.
    fn stat(&self, path: &str) -> Option<(u64, bool)>;

    /// File size for quota accounting. 0 when absent (callers charge net delta).
    fn file_size(&self, path: &str) -> u64;

    /// Whole-file copy for the async-read handle table. Empty when absent.
    fn read_to_vec(&self, path: &str) -> Vec<u8>;

    /// Create-or-truncate write. Authorization is the dispatcher's job
    /// (AccessTable); backends may still enforce structural rules.
    fn write(&mut self, path: &str, content: &[u8]) -> bool;
    /// Positional bounded read. Backends that do not support it return 0.
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> usize {
        let _ = (path, offset, buf);
        0
    }

    /// Positional bounded write. Backends that do not support it return false.
    fn write_at(&mut self, path: &str, offset: u64, content: &[u8]) -> bool {
        let _ = (path, offset, content);
        false
    }

    /// Flush dirty pages for a path to the block device.
    fn sync(&mut self, path: &str) -> bool {
        let _ = path;
        false
    }

    fn append(&mut self, path: &str, content: &[u8]) -> bool;
    /// mkdir -p semantics on FAT; single-level create on RamFS.
    fn mkdir(&mut self, path: &str) -> bool;

    /// Remove an EMPTY directory (POSIX ENOTDIR/ENOTEMPTY semantics).
    fn rmdir(&mut self, path: &str) -> bool;

    /// Remove a regular FILE (directories are rejected).
    fn unlink(&mut self, path: &str) -> bool;

    /// Recursive delete (`rm -r`). Backends that do not support it return false.
    fn rmdir_recursive(&mut self, path: &str) -> bool;

    /// Atomic no-replace rename from `old` to `new`.
    ///
    /// Backends that do not support it return false; the `/srv` backend
    /// (CellosFS Native) implements it.
    fn rename_no_replace(&mut self, old: &str, new: &str) -> bool {
        let _ = (old, new);
        false
    }

    // ── D03 conditional save. Defaults: unsupported (FAT32, bootfs, RamFS). ──

    /// Staging object path for transaction `id`, inside this backend; `None`
    /// when the backend has no conditional publication.
    fn stage_path(&self, id: u32) -> Option<alloc::string::String> {
        let _ = id;
        None
    }

    /// Exact identity of `path`; `None` on error or when unsupported.
    fn observe(&self, path: &str) -> Option<api::ipc::Observation> {
        let _ = path;
        None
    }

    /// Create the empty staging object exclusively. Not synced: staged bytes
    /// become persistent only with the commit's sync.
    fn stage_create(&mut self, stage: &str) -> bool {
        let _ = stage;
        false
    }

    /// Positional write into a staging object, not synced.
    fn stage_write(&mut self, stage: &str, offset: u64, bytes: &[u8]) -> bool {
        let _ = (stage, offset, bytes);
        false
    }

    /// Atomically check that `stage` is still `sealed` and `dst` still matches
    /// `expect`, then publish `stage` at `dst` and sync.
    fn publish_if(&mut self, stage: &str, sealed: (u64, u64), dst: &str,
        expect: api::ipc::SaveExpect) -> api::ipc::SaveOutcome
    {
        let _ = (stage, sealed, dst, expect);
        api::ipc::SaveOutcome::NotPublished(api::ipc::save_refusal::REFUSED)
    }
}
