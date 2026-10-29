//! `VfsManager` — mount table plus the cross-cutting service state (quota,
//! access control, async-read handles, grant handle table).
//!
//! Backend routing is fully encapsulated here: dispatch code calls these
//! delegates and never inspects path prefixes itself.

mod change_marks;
mod conditional_save;
mod listing;
mod owned_state;
mod state_transfer;
#[cfg(feature = "test-hooks")]
pub(crate) mod read_path_selftest;
#[cfg(test)]
mod read_path_tests;
#[cfg(test)]
mod tests;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::access::AccessTable;
use crate::backend_bin_overlay::BinOverlay;
use crate::backend::FsBackend;
use crate::backend_bootfs::BootFsProxy;
use crate::backend_cellosfs::CellosFsBackend;
use crate::backend_fat::FatBackend;
use crate::backend_ramfs::RamFsBackend;
use crate::caller::Caller;
use crate::dirs::DirTable;
use crate::file_handles::FileHandleTable;
use crate::handle_table::HandleTable;
use crate::mount::MountTable;
use crate::namespace::NamespaceLedger;
use crate::pending::PendingTable;
use crate::quota::QuotaTracker;
#[derive(Clone, Copy)]
pub(crate) struct WatchedOwner {
    pub principal: Caller,
    pub root_tid: usize,
    pub token: u64,
}
pub struct VfsManager {
    pub(crate) mounts: MountTable,
    /// Boot-image configuration fallback; runtime files may override only
    /// services/autoload, never the kernel's already-applied system config.
    boot_config: BootFsProxy,
    pub handles: HandleTable,
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
    /// S9-4c first-trust challenges (`trust::first`).
    pub(crate) trust_challenges: crate::trust::Challenges,
    watched_owners: BTreeMap<(u64, u64), WatchedOwner>,
    cancelled_owner_watch_tokens: Vec<u64>,
}

impl VfsManager {
    pub fn new() -> Self {
        let mut mounts = MountTable::new();
        let ram = mounts.add_backend(Box::new(RamFsBackend::new()));
        // FAT32 P1 remains the interoperability/config override volume.
        // Native P4/P5 require explicit offline provisioning; runtime never formats.
        let fat = mounts.add_backend(Box::new(FatBackend::mount(
            "/mnt/sd",
            api::disk::PART_FAT32_BASE_LBA,
        )));
        // /data (P4): CellosFS Native (pure-Rust, power-loss resilient).
        // Available on all architectures with zero external C toolchain dependencies.
        let data_fs = mounts.add_backend(Box::new(CellosFsBackend::mount(
            "/data",
            api::disk::PART_LFS_BASE_LBA,
            api::disk::PART_LFS_SECTORS,
        )));
        // /bin overlay: VIFS1 ramdisk (bootstrap cells) unioned with the on-disk
        // FAT cell-store (non-bootstrap cells migrated off the raw P2 table).
        let binov = mounts.add_backend(Box::new(BinOverlay::new(
            api::disk::PART_CELLSTORE_BASE_LBA,
        )));
        // Longest prefix wins: the specific mounts shadow the read-only root.
        mounts.mount("/", ram);
        mounts.mount("/tmp", ram);
        mounts.mount("/data", data_fs);
        mounts.mount("/mnt/sd", fat);
        mounts.mount("/bin", binov);
        // /srv P5: existing native V1 volume, not an admitted durable journal.
        let srv_fs = mounts.add_backend(Box::new(CellosFsBackend::mount(
            "/srv",
            api::disk::PART_SRV_BASE_LBA,
            api::disk::PART_SRV_SECTORS,
        )));
        mounts.mount("/srv", srv_fs);

        Self {
            mounts,
            boot_config: BootFsProxy::new(),
            handles: HandleTable::new(),
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
            trust_challenges: crate::trust::Challenges::new(),
            watched_owners: BTreeMap::new(),
            cancelled_owner_watch_tokens: Vec::new(),
        }
    }

    /// Consult after authorization and again after reads; never a durability claim.
    pub fn healthy(&self, path: &str) -> bool {
        let backend = self.config_source(path).map(|(backend, _)| backend)
            .or_else(|| self.mounts.backend(path));
        backend.is_some_and(|b| b.health() == crate::backend::BackendHealth::Ready)
    }

    pub fn get_file_ptr(&self, path: &str) -> Option<(usize, usize)> {
        self.mounts.backend(path)?.get_file_ptr(path)
    }

    pub fn list_dir(&self, path: &str, out: &mut [u8]) -> usize {
        if !self.healthy(path) { return 0; }
        let mut pos = self
            .mounts
            .backend(path)
            .map(|b| b.list(path, out))
            .unwrap_or(0);
        if !self.healthy(path) { return 0; }

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
            "/etc/cellos" => &["system.toml", "services.toml", "autoload.toml"],
            _ => &[],
        };
        for name in configured {
            let full_path = alloc::format!("{}/{}", path.trim_end_matches('/'), name);
            let Some((_, directory)) = self.stat(&full_path) else {
                continue;
            };
            if out[..pos].split(|&byte| byte == b'\n').any(|entry| {
                entry.get(2..) == Some(name.as_bytes())
            }) {
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
        pos
    }

    pub fn stat(&self, path: &str) -> Option<(u64, bool)> {
        if !self.healthy(path) { return None; }
        if let Some((backend, source)) = self.config_source(path) {
            return backend.stat(source);
        }
        let result = self.mounts.backend(path).and_then(|b| b.stat(path));
        if !self.healthy(path) { return None; }
        if result.is_some() { return result; }
        if self.mounts.is_mount_ancestor(path) {
            return Some((0, true));
        }
        if matches!(path, "/etc" | "/etc/cellos") {
            return Some((0, true));
        }
        None
    }

    pub fn is_mount_ancestor(&self, path: &str) -> bool {
        self.mounts.is_mount_ancestor(path)
    }

    pub fn is_mount_point(&self, path: &str) -> bool {
        self.mounts.is_mount_point(path)
    }

    pub fn file_size(&self, path: &str) -> u64 {
        if let Some((backend, source)) = self.config_source(path) {
            return backend.file_size(source);
        }
        self.mounts
            .backend(path)
            .map(|b| b.file_size(path))
            .unwrap_or(0)
    }

    pub fn read_to_vec(&self, path: &str) -> Vec<u8> {
        if let Some((backend, source)) = self.config_source(path) {
            return backend.read_to_vec(source);
        }
        self.mounts
            .backend(path)
            .map(|b| b.read_to_vec(path))
            .unwrap_or_default()
    }

    pub fn write(&mut self, path: &str, content: &[u8]) -> bool {
        let created = self.would_create(path);
        let ok = self
            .mounts
            .backend_mut(path)
            .map(|b| b.write(path, content))
            .unwrap_or(false);
        self.mark_content(path, created);
        ok
    }

    pub fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> usize {
        if let Some((backend, source)) = self.config_source(path) {
            return backend.read_at(source, offset, buf);
        }
        self.mounts
            .backend(path)
            .map(|b| b.read_at(path, offset, buf))
            .unwrap_or(0)
    }

    pub fn write_at(&mut self, path: &str, offset: u64, content: &[u8]) -> bool {
        let ok = self
            .mounts
            .backend_mut(path)
            .map(|b| b.write_at(path, offset, content))
            .unwrap_or(false);
        self.mark_content(path, false);
        ok
    }

    pub fn sync(&mut self, path: &str) -> bool {
        self.mounts
            .backend_mut(path)
            .map(|b| b.sync(path))
            .unwrap_or(false)
    }

    pub fn append(&mut self, path: &str, content: &[u8]) -> bool {
        let created = self.would_create(path);
        let ok = self
            .mounts
            .backend_mut(path)
            .map(|b| b.append(path, content))
            .unwrap_or(false);
        self.mark_content(path, created);
        ok
    }

    pub fn mkdir(&mut self, path: &str) -> bool {
        let created = self.first_created_dir(path);
        let ok = self
            .mounts
            .backend_mut(path)
            .map(|b| b.mkdir(path))
            .unwrap_or(false);
        if let Some(created) = created {
            self.mark_binding(&created);
        }
        ok
    }

    pub fn rmdir(&mut self, path: &str) -> bool {
        let ok = self
            .mounts
            .backend_mut(path)
            .map(|b| b.rmdir(path))
            .unwrap_or(false);
        self.mark_tree(path);
        ok
    }

    pub fn unlink(&mut self, path: &str) -> bool {
        let ok = self
            .mounts
            .backend_mut(path)
            .map(|b| b.unlink(path))
            .unwrap_or(false);
        self.mark_binding(path);
        ok
    }

    pub fn rmdir_recursive(&mut self, path: &str) -> bool {
        let ok = self
            .mounts
            .backend_mut(path)
            .map(|b| b.rmdir_recursive(path))
            .unwrap_or(false);
        self.mark_tree(path);
        ok
    }

    pub fn rename_no_replace(&mut self, old: &str, new: &str) -> bool {
        let (Some(old_backend), Some(new_backend)) = (self.mounts.backend(old), self.mounts.backend(new)) else { return false; };
        if !core::ptr::eq(old_backend, new_backend) || !self.healthy(old) || !self.healthy(new) { return false; }
        let ok = self
            .mounts
            .backend_mut(old)
            .map(|b| b.rename_no_replace(old, new))
            .unwrap_or(false);
        self.mark_binding(old);
        self.mark_binding(new);
        ok
    }

    /// Only the three public configuration paths use this source rule. Secrets
    /// and C2C configuration elsewhere in /etc keep their existing routing.
    fn config_source<'a>(&'a self, path: &'a str) -> Option<(&'a dyn FsBackend, &'a str)> {
        let persistent = match path {
            "/etc/cellos/system.toml" => return Some((&self.boot_config, path)),
            "/etc/cellos/services.toml" => "/mnt/sd/etc/cellos/services.toml",
            "/etc/cellos/autoload.toml" => "/mnt/sd/etc/cellos/autoload.toml",
            _ => return None,
        };
        if let Some(backend) = self.mounts.backend(persistent) {
            // Presence, not nonempty content, selects an override. Invalid or
            // empty runtime files must be diagnosed rather than silently ignored.
            if backend.stat(persistent).is_some() {
                return Some((backend, persistent));
            }
        }
        Some((&self.boot_config, path))
    }
}
