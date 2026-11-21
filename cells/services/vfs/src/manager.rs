//! `VfsManager` — mount table plus the cross-cutting service state (quota,
//! access control, async-read handles, grant handle table).
//!
//! Backend routing is fully encapsulated here: dispatch code calls these
//! delegates and never inspects path prefixes itself.

mod change_marks;
mod conditional_save;
mod disks;
mod listing;
mod owned_state;
#[cfg(feature = "test-hooks")]
pub(crate) mod read_path_selftest;
#[cfg(test)]
mod read_path_tests;
#[cfg(test)]
mod tests;

use alloc::boxed::Box;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;

use crate::access::AccessTable;
use crate::backend::FsBackend;
use crate::backend_bootfs::BootFsProxy;
use crate::backend_ramfs::RamFsBackend;
use crate::dirs::DirTable;
use crate::file_handles::FileHandleTable;
use crate::fs_error::FsError;
use crate::mount::MountTable;
use crate::namespace::NamespaceLedger;
use crate::pending::PendingTable;
use crate::quota::QuotaTracker;
pub struct VfsManager {
    pub(crate) mounts: MountTable,
    /// Boot-image configuration fallback; runtime files may override only
    /// services/autoload, never the kernel's already-applied system config.
    boot_config: BootFsProxy,
    pub quota: QuotaTracker,
    pub access: AccessTable,
    pub pending: PendingTable,
    pub files: FileHandleTable,
    /// Directory capabilities. Deliberately not serialised across a hot-swap —
    /// see `dirs::lifecycle`, where the reasoning for that lives.
    pub dirs: DirTable,
    pub ledger: NamespaceLedger,
    /// D03 conditional-save transactions.
    pub(crate) saves: crate::save::SaveTable,
    /// `(cell_id, generation)` of the owners whose death `serve` watches.
    watched_owners: BTreeSet<(u64, u64)>,
}

impl VfsManager {
    /// Discover the block devices, then mount RamFS at `/` and `/tmp`, the
    /// primary disk's partitions and each secondary disk (`manager::disks`).
    pub fn new() -> Self {
        let mut mounts = MountTable::new();
        let ram = mounts.add_backend(Box::new(RamFsBackend::new()));
        // Longest prefix wins: the disk mounts shadow the read-only root.
        mounts.mount("/", ram);
        mounts.mount("/tmp", ram);
        disks::mount_disks(
            &mut mounts,
            disks::disk_mounts(&crate::blk_router::discover()),
        );

        Self {
            mounts,
            boot_config: BootFsProxy::new(),
            // test-hooks: 1.1 KiB quota so vfs-test can hit the limit with
            // 400-byte chunks (must fit within the 512-byte IPC buffer).
            #[cfg(feature = "test-hooks")]
            quota: QuotaTracker::with_limit(1100),
            #[cfg(not(feature = "test-hooks"))]
            quota: QuotaTracker::new(),
            access: AccessTable::new(),
            pending: PendingTable::new(),
            files: FileHandleTable::new(),
            dirs: DirTable::new(),
            ledger: NamespaceLedger::new(),
            saves: crate::save::SaveTable::new(),
            watched_owners: BTreeSet::new(),
        }
    }

    /// Consult after authorization and again after reads; never a durability claim.
    pub fn healthy(&self, path: &str) -> bool {
        let backend = self
            .config_source(path)
            .map(|(backend, _)| backend)
            .or_else(|| self.mounts.backend(path));
        backend.is_some_and(|b| b.health() == crate::backend::BackendHealth::Ready)
    }

    /// The backend's listing of `path` merged with the mount-table entries.
    /// A path the backend does not hold as a directory lists only those; a
    /// device failure fails the listing rather than shortening it.
    pub fn list_dir(&self, path: &str, out: &mut [u8]) -> Result<usize, FsError> {
        if !self.healthy(path) {
            return Err(FsError::Unavailable);
        }
        let mut pos = match self.mounts.backend(path).map(|b| b.list(path, out)) {
            Some(Ok(n)) => n,
            Some(Err(error @ (FsError::Io | FsError::Indeterminate))) => return Err(error),
            Some(Err(_)) | None => 0,
        };
        if !self.healthy(path) {
            return Err(FsError::Unavailable);
        }

        let cap = out.len().min(480);
        for child in self.mounts.direct_mount_children(path) {
            let child_b = child.as_bytes();
            let entry_len = 2 + child_b.len() + 1; // "d:" + name + "\n"
            if pos + entry_len > cap {
                break;
            }

            let mut already_present = false;
            let current = &out[..pos];
            for line in current.split(|&b| b == b'\n') {
                if let Some(name) = line
                    .strip_prefix(b"d:")
                    .or_else(|| line.strip_prefix(b"f:"))
                {
                    if name == child_b {
                        already_present = true;
                        break;
                    }
                }
            }

            if !already_present {
                out[pos..pos + 2].copy_from_slice(b"d:");
                out[pos + 2..pos + 2 + child_b.len()].copy_from_slice(child_b);
                out[pos + 2 + child_b.len()] = b'\n';
                pos += entry_len;
            }
        }
        let configured: &[&str] = match path.trim_end_matches('/') {
            "" => &["etc"],
            "/etc" => &["cellos"],
            "/etc/cellos" => &[
                "system.toml",
                "services.toml",
                "autoload.toml",
                "cluster.cfg",
                "c2c-exports.cfg",
            ],
            _ => &[],
        };
        for name in configured {
            let full_path = alloc::format!("{}/{}", path.trim_end_matches('/'), name);
            let Ok(Some((_, directory))) = self.stat(&full_path) else {
                continue;
            };
            if out[..pos]
                .split(|&byte| byte == b'\n')
                .any(|entry| entry.get(2..) == Some(name.as_bytes()))
            {
                continue;
            }
            let needed = 2 + name.len() + 1;
            if pos + needed > out.len().min(480) {
                break;
            }
            out[pos..pos + 2].copy_from_slice(if directory { b"d:" } else { b"f:" });
            pos += 2;
            out[pos..pos + name.len()].copy_from_slice(name.as_bytes());
            pos += name.len();
            out[pos] = b'\n';
            pos += 1;
        }
        Ok(pos)
    }

    /// `Ok(None)` when nothing exists at `path`. Directories the mount table
    /// supplies exist even when their backend failed to answer.
    pub fn stat(&self, path: &str) -> Result<Option<(u64, bool)>, FsError> {
        if !self.healthy(path) {
            return Err(FsError::Unavailable);
        }
        if let Some((backend, source)) = self.config_source(path) {
            return backend.stat(source);
        }
        let result = self.mounts.backend(path).map_or(Ok(None), |b| b.stat(path));
        if !self.healthy(path) {
            return Err(FsError::Unavailable);
        }
        if let Ok(Some(hit)) = result {
            return Ok(Some(hit));
        }
        if self.mounts.is_mount_ancestor(path) {
            return Ok(Some((0, true)));
        }
        if matches!(path, "/etc" | "/etc/cellos") {
            return Ok(Some((0, true)));
        }
        result
    }

    pub fn is_mount_ancestor(&self, path: &str) -> bool {
        self.mounts.is_mount_ancestor(path)
    }

    pub fn is_mount_point(&self, path: &str) -> bool {
        self.mounts.is_mount_point(path)
    }

    pub fn file_size(&self, path: &str) -> Result<u64, FsError> {
        if let Some((backend, source)) = self.config_source(path) {
            return backend.file_size(source);
        }
        self.mounts
            .backend(path)
            .ok_or(FsError::Unavailable)?
            .file_size(path)
    }

    pub fn read_to_vec(&self, path: &str) -> Result<Vec<u8>, FsError> {
        if let Some((backend, source)) = self.config_source(path) {
            return backend.read_to_vec(source);
        }
        self.mounts
            .backend(path)
            .ok_or(FsError::Unavailable)?
            .read_to_vec(path)
    }

    pub fn write(&mut self, path: &str, content: &[u8]) -> Result<(), FsError> {
        let created = self.would_create(path);
        let result = self
            .mounts
            .backend_mut(path)
            .map_or(Err(FsError::Unavailable), |b| b.write(path, content));
        self.mark_content(path, created);
        result
    }

    pub fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        if let Some((backend, source)) = self.config_source(path) {
            return backend.read_at(source, offset, buf);
        }
        self.mounts
            .backend(path)
            .ok_or(FsError::Unavailable)?
            .read_at(path, offset, buf)
    }

    pub fn write_at(&mut self, path: &str, offset: u64, content: &[u8]) -> Result<(), FsError> {
        let result = self
            .mounts
            .backend_mut(path)
            .map_or(Err(FsError::Unavailable), |b| {
                b.write_at(path, offset, content)
            });
        self.mark_content(path, false);
        result
    }

    pub fn sync(&mut self, path: &str) -> Result<(), FsError> {
        self.mounts
            .backend_mut(path)
            .map_or(Err(FsError::Unavailable), |b| b.sync(path))
    }

    pub fn append(&mut self, path: &str, content: &[u8]) -> Result<(), FsError> {
        let created = self.would_create(path);
        let result = self
            .mounts
            .backend_mut(path)
            .map_or(Err(FsError::Unavailable), |b| b.append(path, content));
        self.mark_content(path, created);
        result
    }

    pub fn mkdir(&mut self, path: &str) -> Result<(), FsError> {
        let created = self.first_created_dir(path);
        let result = self
            .mounts
            .backend_mut(path)
            .map_or(Err(FsError::Unavailable), |b| b.mkdir(path));
        if let Some(created) = created {
            self.mark_binding(&created);
        }
        result
    }

    pub fn rmdir(&mut self, path: &str) -> Result<(), FsError> {
        let result = self
            .mounts
            .backend_mut(path)
            .map_or(Err(FsError::Unavailable), |b| b.rmdir(path));
        self.mark_tree(path);
        result
    }

    pub fn unlink(&mut self, path: &str) -> Result<(), FsError> {
        let result = self
            .mounts
            .backend_mut(path)
            .map_or(Err(FsError::Unavailable), |b| b.unlink(path));
        self.mark_binding(path);
        result
    }

    pub fn rmdir_recursive(&mut self, path: &str) -> Result<(), FsError> {
        let result = self
            .mounts
            .backend_mut(path)
            .map_or(Err(FsError::Unavailable), |b| b.rmdir_recursive(path));
        self.mark_tree(path);
        result
    }

    /// `Unsupported` across two backends: no backend renames between mounts.
    pub fn rename_no_replace(&mut self, old: &str, new: &str) -> Result<(), FsError> {
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
            .map_or(Err(FsError::Unavailable), |b| b.rename_no_replace(old, new));
        self.mark_binding(old);
        self.mark_binding(new);
        result
    }

    /// A runtime file under `/mnt/sd/etc/cellos` overrides its public
    /// `/etc/cellos` path when present. The boot TOML files otherwise come from
    /// the boot image; the C2C files keep their existing mount routing, which is
    /// where the K1 oracle fixture lives (RamFS), so `None` selects it.
    fn config_source<'a>(&'a self, path: &'a str) -> Option<(&'a dyn FsBackend, &'a str)> {
        let (persistent, boot_image_default) = match path {
            "/etc/cellos/system.toml" => return Some((&self.boot_config, path)),
            "/etc/cellos/services.toml" => ("/mnt/sd/etc/cellos/services.toml", true),
            "/etc/cellos/autoload.toml" => ("/mnt/sd/etc/cellos/autoload.toml", true),
            "/etc/cellos/cluster.key" => ("/mnt/sd/etc/cellos/cluster.key", false),
            "/etc/cellos/cluster.cfg" => ("/mnt/sd/etc/cellos/cluster.cfg", false),
            "/etc/cellos/c2c-exports.cfg" => ("/mnt/sd/etc/cellos/c2c-exports.cfg", false),
            _ => return None,
        };
        if let Some(backend) = self.mounts.backend(persistent) {
            // Presence, not nonempty content, selects an override. Invalid or
            // empty runtime files must be diagnosed rather than silently ignored.
            if matches!(backend.stat(persistent), Ok(Some(_))) {
                return Some((backend, persistent));
            }
        }
        boot_image_default.then_some((&self.boot_config as &dyn FsBackend, path))
    }
}
