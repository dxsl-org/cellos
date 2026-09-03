//! `VfsManager` — mount table plus the cross-cutting service state (quota,
//! access control, async-read handles, grant handle table).
//!
//! Backend routing is fully encapsulated here: dispatch code calls these
//! delegates and never inspects path prefixes itself.

mod owned_state;
mod state_transfer;
#[cfg(test)]
mod tests;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::access::AccessTable;
use crate::backend_bin_overlay::BinOverlay;
use crate::backend_fat::FatBackend;
#[cfg(feature = "littlefs")]
use crate::backend_littlefs::LittlefsBackend;
use crate::backend_ramfs::RamFsBackend;
use crate::backend_redoxfs::RedoxFsBackend;
use crate::caller::Caller;
use crate::dirs::DirTable;
use crate::file_handles::FileHandleTable;
use crate::handle_table::HandleTable;
use crate::mount::MountTable;
use crate::pending::PendingTable;
use crate::namespace::NamespaceLedger;
use crate::quota::QuotaTracker;
#[derive(Clone, Copy)]
pub(crate) struct WatchedOwner {
    pub principal: Caller,
    pub root_tid: usize,
    pub token: u64,
}
pub struct VfsManager {
    pub(crate) mounts: MountTable,
    pub handles: HandleTable,
    pub quota: QuotaTracker,
    pub access: AccessTable,
    pub pending: PendingTable,
    pub files: FileHandleTable,
    /// Directory capabilities. Deliberately not serialised across a hot-swap —
    /// see `dirs::lifecycle`, where the reasoning for that lives.
    pub dirs: DirTable,
    pub ledger: NamespaceLedger,
    watched_owners: BTreeMap<(u64, u64), WatchedOwner>,
    cancelled_owner_watch_tokens: Vec<u64>,
}

impl VfsManager {
    pub fn new() -> Self {
        let mut mounts = MountTable::new();
        let ram = mounts.add_backend(Box::new(RamFsBackend::new()));
        // FAT32 (P1) is the SD-card/PC interop volume since P04 — /data moved
        // to littlefs (P4), which survives power cuts (FAT has no journal).
        let fat = mounts.add_backend(Box::new(FatBackend::mount(
            "/mnt/sd",
            api::disk::PART_FAT32_BASE_LBA,
        )));
        // /data (littlefs, P4) is gated on the `littlefs` feature: builds without a
        // bare-metal C toolchain (x86_64/aarch64) omit it — the persistent /data
        // volume is simply absent there, which boot-to-shell does not require.
        #[cfg(feature = "littlefs")]
        let lfs = mounts.add_backend(Box::new(LittlefsBackend::mount("/data")));
        // /bin overlay: VIFS1 ramdisk (bootstrap cells) unioned with the on-disk
        // FAT cell-store (non-bootstrap cells migrated off the raw P2 table).
        let binov = mounts.add_backend(Box::new(BinOverlay::new(
            api::disk::PART_CELLSTORE_BASE_LBA,
        )));
        // Longest prefix wins: the specific mounts shadow the read-only root.
        mounts.mount("/", ram);
        mounts.mount("/tmp", ram);
        #[cfg(feature = "littlefs")]
        mounts.mount("/data", lfs);
        mounts.mount("/mnt/sd", fat);
        mounts.mount("/bin", binov);
        // /srv: RedoxFS CoW B-tree filesystem on MBR partition P5.
        // Degrades gracefully to empty/false if P5 is unformatted (see
        // docs/specs/09b-vfs-native-fs-adr.md and scripts/mksrv-img.sh).
        let srv = mounts.add_backend(Box::new(RedoxFsBackend::mount("/srv")));
        mounts.mount("/srv", srv);

        Self {
            mounts,
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
            watched_owners: BTreeMap::new(),
            cancelled_owner_watch_tokens: Vec::new(),
        }
    }

    pub fn get_file_ptr(&self, path: &str) -> Option<(usize, usize)> {
        self.mounts.backend(path)?.get_file_ptr(path)
    }

    pub fn list_dir(&self, path: &str, out: &mut [u8]) -> usize {
        self.mounts.backend(path).map(|b| b.list(path, out)).unwrap_or(0)
    }

    pub fn stat(&self, path: &str) -> Option<(u64, bool)> {
        self.mounts.backend(path)?.stat(path)
    }

    pub fn is_mount_ancestor(&self, path: &str) -> bool {
        self.mounts.is_mount_ancestor(path)
    }

    pub fn file_size(&self, path: &str) -> u64 {
        self.mounts.backend(path).map(|b| b.file_size(path)).unwrap_or(0)
    }

    pub fn read_to_vec(&self, path: &str) -> Vec<u8> {
        self.mounts.backend(path).map(|b| b.read_to_vec(path)).unwrap_or_default()
    }

    pub fn write(&mut self, path: &str, content: &[u8]) -> bool {
        self.mounts.backend_mut(path).map(|b| b.write(path, content)).unwrap_or(false)
    }

    pub fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> usize {
        self.mounts.backend(path).map(|b| b.read_at(path, offset, buf)).unwrap_or(0)
    }

    pub fn write_at(&mut self, path: &str, offset: u64, content: &[u8]) -> bool {
        self.mounts.backend_mut(path).map(|b| b.write_at(path, offset, content)).unwrap_or(false)
    }

    pub fn sync(&mut self, path: &str) -> bool {
        self.mounts.backend_mut(path).map(|b| b.sync(path)).unwrap_or(false)
    }

    pub fn append(&mut self, path: &str, content: &[u8]) -> bool {
        self.mounts.backend_mut(path).map(|b| b.append(path, content)).unwrap_or(false)
    }

    pub fn mkdir(&mut self, path: &str) -> bool {
        self.mounts.backend_mut(path).map(|b| b.mkdir(path)).unwrap_or(false)
    }

    pub fn rmdir(&mut self, path: &str) -> bool {
        self.mounts.backend_mut(path).map(|b| b.rmdir(path)).unwrap_or(false)
    }

    pub fn unlink(&mut self, path: &str) -> bool {
        self.mounts.backend_mut(path).map(|b| b.unlink(path)).unwrap_or(false)
    }

    pub fn rmdir_recursive(&mut self, path: &str) -> bool {
        self.mounts.backend_mut(path).map(|b| b.rmdir_recursive(path)).unwrap_or(false)
    }

    pub fn rename_no_replace(&mut self, old: &str, new: &str) -> bool {
        self.mounts.backend_mut(old).map(|b| b.rename_no_replace(old, new)).unwrap_or(false)
    }
}
