//! Mount-point registry for the VFS service (specs/09-vfs.md v0.5 §2).
//!
//! Longest-matching-prefix wins, with boundary-aware matching: `/data` matches
//! `/data` and `/data/x` but NOT `/dataX`. Entries reference backends by index
//! so one backend instance can serve multiple mount points (RamFS serves both
//! the read-only `/` catalog and writable `/tmp`).

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::backend::FsBackend;

struct MountEntry {
    /// Owned: secondary-disk mount points (`/mnt/disk/<name>`) are named at runtime.
    prefix: Box<str>,
    backend: usize,
}

pub struct MountTable {
    backends: Vec<Box<dyn FsBackend>>,
    entries: Vec<MountEntry>,
}

impl MountTable {
    pub fn new() -> Self {
        Self {
            backends: Vec::new(),
            entries: Vec::new(),
        }
    }

    /// Register a backend; returns its index for use in `mount()`.
    pub fn add_backend(&mut self, backend: Box<dyn FsBackend>) -> usize {
        self.backends.push(backend);
        self.backends.len() - 1
    }

    /// Bind `prefix` to a backend index.
    ///
    /// Mount points carry no write-permission flag: write authorization has exactly
    /// two sources, and a third half-wired one would drift from them silently.
    /// Policy ("may this cell write this path") is `AccessTable`, consulted in
    /// `dispatch`; structure ("is this backend writable at all") is the backend
    /// itself — e.g. `BinOverlay` returns `false` from every mutating method.
    pub fn mount(&mut self, prefix: &str, backend: usize) {
        self.entries.push(MountEntry { prefix: prefix.into(), backend });
    }

    /// Boundary-aware prefix match: the next char after the prefix must be `/`
    /// (or the path equals the prefix). Root `/` matches every absolute path.
    fn prefix_matches(prefix: &str, path: &str) -> bool {
        if prefix == "/" {
            return path.starts_with('/');
        }
        match path.strip_prefix(prefix) {
            Some("") => true,
            Some(rest) => rest.starts_with('/'),
            None => false,
        }
    }

    fn resolve_idx(&self, path: &str) -> Option<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| Self::prefix_matches(&e.prefix, path))
            .max_by_key(|(_, e)| e.prefix.len())
            .map(|(i, _)| self.entries[i].backend)
    }

    /// Whether `path` is a strict directory ancestor of a registered mount.
    ///
    /// Handle-addressed clients traverse one component at a time. Such a client
    /// must be able to derive `/mnt` before it can reach a `/mnt/sd` mount, even
    /// when the root backend does not materialize that structural directory.
    pub fn is_mount_ancestor(&self, path: &str) -> bool {
        if path == "/" || !path.starts_with('/') || path.ends_with('/') {
            return false;
        }
        self.entries.iter().any(|entry| {
            entry.prefix.len() > path.len()
                && entry
                    .prefix
                    .strip_prefix(path)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        })
    }

    /// Whether `path` is an exact registered mount point (e.g. `/`, `/tmp`, `/data`, `/srv`, `/mnt/sd`).
    pub fn is_mount_point(&self, path: &str) -> bool {
        let trimmed = if path == "/" {
            "/"
        } else {
            path.trim_end_matches('/')
        };
        self.entries.iter().any(|entry| &*entry.prefix == trimmed)
    }

    /// Enumerate direct child directory names under `path` that are mount points
    /// or directory ancestors of registered mounts.
    pub fn direct_mount_children(&self, path: &str) -> Vec<&str> {
        let parent = if path == "/" {
            ""
        } else {
            path.trim_end_matches('/')
        };
        let mut children = Vec::new();
        for entry in &self.entries {
            if &*entry.prefix == "/" {
                continue;
            }
            if let Some(rest) = entry.prefix.strip_prefix(parent) {
                if let Some(after_slash) = rest.strip_prefix('/') {
                    let child = after_slash.split('/').next().unwrap_or("");
                    if !child.is_empty() && !children.contains(&child) {
                        children.push(child);
                    }
                }
            }
        }
        children
    }

    pub fn backend(&self, path: &str) -> Option<&dyn FsBackend> {
        self.resolve_idx(path).map(|i| self.backends[i].as_ref())
    }

    pub fn backend_mut(&mut self, path: &str) -> Option<&mut (dyn FsBackend + 'static)> {
        let i = self.resolve_idx(path)?;
        Some(self.backends[i].as_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::MountTable;

    #[test]
    fn runtime_named_secondary_mounts_resolve_and_list_under_mnt() {
        let mut table = MountTable::new();
        for (index, prefix) in ["/", "/mnt/sd", "/mnt/disk/pci-00-1f-2"].into_iter().enumerate() {
            table.mount(&alloc::format!("{prefix}"), index);
        }
        assert_eq!(table.resolve_idx("/mnt/disk/pci-00-1f-2/f"), Some(2));
        assert_eq!(table.resolve_idx("/mnt/disk/pci-00-1f-20"), Some(0));
        assert_eq!(table.resolve_idx("/mnt/sd/f"), Some(1));
        assert!(table.is_mount_ancestor("/mnt"));
        assert!(table.is_mount_ancestor("/mnt/disk"));
        assert!(table.is_mount_point("/mnt/disk/pci-00-1f-2/"));
        assert_eq!(table.direct_mount_children("/mnt"), ["sd", "disk"]);
        assert_eq!(table.direct_mount_children("/mnt/disk"), ["pci-00-1f-2"]);
    }
}
