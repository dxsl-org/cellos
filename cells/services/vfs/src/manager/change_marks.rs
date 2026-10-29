//! Mutation marks for open handles (`FileHandleState.modified`, `DirPage`
//! `CHANGED`).
//!
//! Every mutating delegate of [`VfsManager`] reports here, so no request path
//! can reach a backend without marking: the dispatchers for `Write`, `WriteAt`,
//! `Append`, `WriteGrant`, `WriteHandleGrant`, `Unlink`/`UnlinkAt`, `Rename`,
//! `Mkdir`, `Rmdir` and `RmdirRecursive` all end in these delegates.
//!
//! A mutation that reached the backend marks even when the backend reported
//! failure: FAT can fail after writing part of a file and CellosFS reports a
//! possibly-accepted failure, so "failed" is not "unchanged". The cost is one
//! scan of the bounded handle tables (32 per holder) per mutation; no history
//! is kept.

use alloc::string::String;

use crate::manager::VfsManager;
use crate::paths::{may_alias, may_alias_within, parent_of};

/// Runtime configuration files and the public paths whose content they supply
/// (see `VfsManager::config_source`).
const CONFIG_ALIASES: [(&str, &str); 2] = [
    ("/mnt/sd/etc/cellos/services.toml", "/etc/cellos/services.toml"),
    ("/mnt/sd/etc/cellos/autoload.toml", "/etc/cellos/autoload.toml"),
];

impl VfsManager {
    /// Whether writing `path` would add an entry to a directory that a handle
    /// currently names. Stats only when such a handle exists.
    pub(super) fn would_create(&self, path: &str) -> bool {
        parent_of(path).is_some_and(|parent| self.dirs.watches(parent)) && self.stat(path).is_none()
    }

    /// The outermost directory `mkdir path` would create, if it creates any.
    pub(super) fn first_created_dir(&self, path: &str) -> Option<String> {
        if self.stat(path).is_some() {
            return None;
        }
        let mut top = path;
        while let Some(parent) = parent_of(top) {
            if self.stat(parent).is_some() {
                break;
            }
            top = parent;
        }
        Some(String::from(top))
    }

    /// The bytes of `path` changed; `created` when the write added the entry.
    pub(super) fn mark_content(&mut self, path: &str, created: bool) {
        self.files.mark_modified(path);
        for (_, alias) in CONFIG_ALIASES.iter().filter(|(source, _)| may_alias(source, path)) {
            self.files.mark_modified(alias);
        }
        if created {
            if let Some(parent) = parent_of(path) {
                self.dirs.mark_changed(parent);
            }
        }
    }

    /// `path` was added to or removed from its parent directory.
    pub(super) fn mark_binding(&mut self, path: &str) {
        self.mark_content(path, true);
    }

    /// `path` and everything below it were removed.
    pub(super) fn mark_tree(&mut self, path: &str) {
        self.files.mark_modified_within(path);
        for (_, alias) in CONFIG_ALIASES.iter().filter(|(source, _)| may_alias_within(source, path)) {
            self.files.mark_modified(alias);
        }
        self.dirs.mark_changed_within(path);
        self.mark_binding(path);
    }
}
