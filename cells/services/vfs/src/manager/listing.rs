//! Paged directory listing across the mount table (`ListAtPage`).
//!
//! A listing is the backend's own entries followed by the entries the mount
//! table adds: mount points and mount ancestors below the directory, and the
//! public configuration names under `/etc`. Unlike `list_dir`, nothing is capped
//! or dropped: whatever does not fit one page is on the next one.

use alloc::vec::Vec;
use api::dir_name::join_component;
use api::ipc::vfs_dir_page::{push_entry, EntryKind, ENTRY_HEADER_LEN};

use crate::backend::{FsBackend, ListPage};
use crate::manager::VfsManager;

/// Cursor bit marking a position among the mount-table entries; backend
/// cursors stay below `backend::BACKEND_CURSOR_LIMIT`.
const EXTRA_PHASE: u32 = 1 << 31;

/// One entry the mount table contributes to a listing.
struct Extra {
    name: &'static str,
    kind: EntryKind,
    /// The backend lists this name itself; only its kind is overridden.
    in_backend: bool,
}

impl VfsManager {
    /// One page of directory `path`, starting at `cursor`. `None` for anything
    /// the wire reports as `ERR_IO`: not a listable directory, a backend
    /// iteration error, an invalid name, or a stale cursor.
    pub fn list_page(&self, path: &str, cursor: u32, out: &mut [u8]) -> Option<ListPage> {
        let backend = self.mounts.backend(path)?;
        let extras = self.listing_extras(path, backend);
        let (mut written, extra_start) = if cursor & EXTRA_PHASE == 0 {
            let page = if matches!(backend.stat(path), Some((_, true))) {
                backend.list_page(path, cursor, out)?
            } else if cursor == 0 && self.is_synthetic_dir(path) {
                ListPage { written: 0, next: None }
            } else {
                return None;
            };
            if page.next.is_some_and(|next| next & EXTRA_PHASE != 0) {
                return None;
            }
            mark_mount_kinds(&mut out[..page.written], &extras);
            if page.next.is_some() {
                return (page.written > 0).then_some(page);
            }
            (page.written, 0)
        } else {
            (0, (cursor & !EXTRA_PHASE) as usize)
        };
        let pending: Vec<&Extra> = extras.iter().filter(|extra| !extra.in_backend).collect();
        if extra_start > pending.len() {
            return None;
        }
        for (index, extra) in pending.iter().enumerate().skip(extra_start) {
            match push_entry(out, written, extra.kind, extra.name.as_bytes()) {
                Ok(Some(end)) => written = end,
                Ok(None) => {
                    let next = Some(EXTRA_PHASE | index as u32);
                    return (written > 0).then_some(ListPage { written, next });
                }
                Err(_) => return None,
            }
        }
        Some(ListPage { written, next: None })
    }

    /// A directory the mount table supplies although no backend stores it.
    fn is_synthetic_dir(&self, path: &str) -> bool {
        self.is_mount_ancestor(path) || matches!(path, "/etc" | "/etc/cellos")
    }

    /// Mount children of `path` (kind `Mount` for a mount point, `Dir` for a
    /// directory that only leads to one), then the configuration names, the
    /// same set `list_dir` adds.
    fn listing_extras(&self, path: &str, backend: &dyn FsBackend) -> Vec<Extra> {
        let mut extras: Vec<Extra> = Vec::new();
        for name in self.mounts.direct_mount_children(path) {
            let child = join_component(path, name);
            let kind = if self.mounts.is_mount_point(&child) { EntryKind::Mount } else { EntryKind::Dir };
            extras.push(Extra { name, kind, in_backend: backend.stat(&child).is_some() });
        }
        let configured: &[&'static str] = match path {
            "/" => &["etc"],
            "/etc" => &["cellos"],
            "/etc/cellos" => &["system.toml", "services.toml", "autoload.toml"],
            _ => &[],
        };
        for name in configured {
            if extras.iter().any(|extra| extra.name == *name) {
                continue;
            }
            let child = join_component(path, name);
            let Some((_, directory)) = self.stat(&child) else {
                continue;
            };
            let kind = if directory { EntryKind::Dir } else { EntryKind::File };
            extras.push(Extra { name, kind, in_backend: backend.stat(&child).is_some() });
        }
        extras
    }
}

/// Report backend entries that are mount points as `Mount`: the backend sees
/// its own directory there, but every request below it goes to another mount.
fn mark_mount_kinds(page: &mut [u8], extras: &[Extra]) {
    let mut pos = 0;
    while pos + ENTRY_HEADER_LEN <= page.len() {
        let end = pos + ENTRY_HEADER_LEN + usize::from(page[pos + 1]);
        let Some(name) = page.get(pos + ENTRY_HEADER_LEN..end) else {
            return;
        };
        if let Some(extra) = extras.iter().find(|extra| extra.in_backend && extra.name.as_bytes() == name) {
            page[pos] = extra.kind.wire();
        }
        pos = end;
    }
}
