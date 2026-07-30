//! Enumerating a directory tree's files before it is deleted.
//!
//! `FsBackend::rmdir_recursive` returns only a bool, so the bytes it frees have to
//! be measured while the tree still exists. Quota must also be released *per
//! file*, because two files in one tree can have been charged to two different
//! cells — so the walk yields paths and sizes, not a single total.

use alloc::string::String;
use alloc::vec::Vec;

use crate::manager::VfsManager;

/// Every regular file under `path`, as `(full path, size in bytes)`.
///
/// Bounded to `depth` levels of recursion and to a 512-byte listing buffer per
/// level, which is adequate for the directory sizes this filesystem carries. A
/// tree deeper or wider than those bounds is under-reported, which under-releases
/// quota — the safe direction, since over-releasing would hand out free quota.
pub fn files_under(vfs: &VfsManager, path: &str, depth: u8) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    collect(vfs, path, depth, &mut out);
    out
}

fn collect(vfs: &VfsManager, path: &str, depth: u8, out: &mut Vec<(String, u64)>) {
    if depth == 0 {
        return;
    }
    let mut scratch = [0u8; 512];
    let n = vfs.list_dir(path, &mut scratch);
    let listing = core::str::from_utf8(&scratch[..n]).unwrap_or("");
    let base = path.trim_end_matches('/');
    for line in listing.split('\n') {
        if let Some(name) = line.strip_prefix("f:") {
            let child = join(base, name);
            let size = vfs.file_size(&child);
            out.push((child, size));
        } else if let Some(name) = line.strip_prefix("d:") {
            collect(vfs, &join(base, name), depth - 1, out);
        }
    }
}

fn join(base: &str, name: &str) -> String {
    let mut child = String::with_capacity(base.len() + 1 + name.len());
    child.push_str(base);
    child.push('/');
    child.push_str(name);
    child
}
