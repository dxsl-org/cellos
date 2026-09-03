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

/// `Send` bound (Law 7): backends live inside the `GLOBAL_VFS` static Mutex,
/// which requires its contents to be `Send` for the static to be `Sync`.
pub trait FsBackend: Send {
    /// SAS zero-copy pointer to the file's bytes. Only in-memory backends
    /// return `Some` — pointers must stay valid while the VFS cell lives.
    /// Disk backends return `None`; callers fall back to the copy path.
    fn get_file_ptr(&self, path: &str) -> Option<(usize, usize)>;

    /// List directory entries as `d:name\n` / `f:name\n` lines into `out`.
    /// Returns bytes written; 0 when the path is missing or not a directory.
    fn list(&self, path: &str, out: &mut [u8]) -> usize;

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
    /// Backends that do not support it return false (only RedoxFS supports this).
    fn rename_no_replace(&mut self, old: &str, new: &str) -> bool {
        let _ = (old, new);
        false
    }
}
