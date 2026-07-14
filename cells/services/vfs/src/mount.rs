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
    prefix: &'static str,
    backend: usize,
    /// Informational until AccessTable rules are mount-driven (Milestone 2.1-3);
    /// actual write authorization lives in AccessTable + backend structural rules.
    #[allow(dead_code)]
    writable: bool,
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

    pub fn mount(&mut self, prefix: &'static str, backend: usize, writable: bool) {
        self.entries.push(MountEntry {
            prefix,
            backend,
            writable,
        });
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
            .filter(|(_, e)| Self::prefix_matches(e.prefix, path))
            .max_by_key(|(_, e)| e.prefix.len())
            .map(|(i, _)| self.entries[i].backend)
    }

    pub fn backend(&self, path: &str) -> Option<&dyn FsBackend> {
        self.resolve_idx(path).map(|i| self.backends[i].as_ref())
    }

    pub fn backend_mut(&mut self, path: &str) -> Option<&mut (dyn FsBackend + 'static)> {
        let i = self.resolve_idx(path)?;
        Some(self.backends[i].as_mut())
    }
}
