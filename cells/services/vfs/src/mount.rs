#![allow(dead_code)] // reason: write path wired in full VirtIO-FAT phase
//! Mount-point registry for the VFS service.
//!
//! Supports a simple prefix-based lookup: the longest matching mount-point
//! prefix wins.  Currently two mount points are supported:
//!   `/`    → RamFS (read-only embedded catalog)
//!   `/tmp` → volatile RamFS (read-write, no disk backing)

use alloc::string::String;
use alloc::vec::Vec;

/// A single mount-point entry.
pub struct MountEntry {
    pub prefix: String,
    pub writable: bool,
    // In v1.0 all backing is RamFS; Phase 14+ adds FatFS behind an Arc<dyn ViFileSystem>.
}

/// Registry of active mount points.
pub struct MountTable {
    entries: Vec<MountEntry>,
}

impl MountTable {
    pub fn new() -> Self {
        let mut t = Self { entries: Vec::new() };
        // Read-only root: serves /bin/, /etc/, /readme.txt from embedded data.
        t.entries.push(MountEntry { prefix: String::from("/"), writable: false });
        // Writable /tmp: volatile scratch space.
        t.entries.push(MountEntry { prefix: String::from("/tmp"), writable: true });
        t
    }

    /// Return the mount point whose prefix is the longest match for `path`.
    pub fn resolve(&self, path: &str) -> Option<&MountEntry> {
        self.entries
            .iter()
            .filter(|e| path.starts_with(e.prefix.as_str()))
            .max_by_key(|e| e.prefix.len())
    }
}
