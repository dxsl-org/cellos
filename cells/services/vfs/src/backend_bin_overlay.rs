//! `/bin` overlay backend — unions the kernel VIFS1 ramdisk (`BootFsProxy`) with
//! the on-disk FAT cell-store, so `/bin/<cell>` resolves whether the ELF is a
//! bootstrap cell embedded in `kernel_fs.img` OR a non-bootstrap cell migrated to
//! the disk cell-store (G2 loader redesign, phase 03).
//!
//! Read ops try VIFS1 first (bootstrap / trusted-core wins on name collision),
//! then fall back to the cell-store. `list` unions both. Strictly read-only:
//! `/bin` is never writable, so every mutating op fails `Unsupported`.

use alloc::vec::Vec;

use crate::backend::{FsBackend, ListPage};
use crate::backend_bootfs::BootFsProxy;
use crate::backend_fat::FatBackend;
use crate::blk_router::DeviceRef;
use crate::fs_error::FsError;

/// Cursor bit marking a position in the cell-store half of a `/bin` listing.
const STORE_PHASE: u32 = 1 << 30;

pub struct BinOverlay {
    boot: BootFsProxy,
    store: FatBackend,
}

impl BinOverlay {
    /// `store_base_lba`/`store_sectors` = `abi::disk::PART_CELLSTORE_*` on the
    /// primary disk `dev`. The cell-store FAT is mounted with prefix `/bin` so
    /// `/bin/<cell>` strips to `<cell>` at the FAT root — matching the layout
    /// gen_disk writes into the cell-store.
    pub fn new(dev: DeviceRef, store_base_lba: u64, store_sectors: u64) -> Self {
        Self {
            boot: BootFsProxy::new(),
            store: FatBackend::mount("/bin", dev, store_base_lba, store_sectors),
        }
    }

    /// The cell-store's stat; a cell-store that never mounted holds nothing.
    fn store_stat(&self, path: &str) -> Result<Option<(u64, bool)>, FsError> {
        match self.store.stat(path) {
            Err(FsError::Unavailable) => Ok(None),
            result => result,
        }
    }
}

/// VIFS1's answer unless it failed or is `empty`; then the cell-store's. An
/// empty VIFS1 answer stands when the cell-store fails as well.
fn boot_first<T>(
    boot: Result<T, FsError>,
    empty: impl Fn(&T) -> bool,
    store: impl FnOnce() -> Result<T, FsError>,
) -> Result<T, FsError> {
    match boot {
        Ok(value) if !empty(&value) => Ok(value),
        Ok(value) => store().or(Ok(value)),
        Err(_) => store(),
    }
}

impl FsBackend for BinOverlay {
    fn get_file_ptr(&self, path: &str) -> Option<(usize, usize)> {
        self.boot
            .get_file_ptr(path)
            .or_else(|| self.store.get_file_ptr(path))
    }

    fn list(&self, path: &str, out: &mut [u8]) -> Result<usize, FsError> {
        // Union: VIFS1 entries first, then cell-store entries into the remainder.
        // Bootstrap and P2-only cell sets are disjoint by construction, so no
        // dedup is needed (a rare collision would list the name twice — benign).
        // Each half alone may be missing; only both missing (or a half that
        // failed to read) fails the listing.
        let n = match self.boot.list(path, out) {
            Ok(n) => Some(n),
            Err(FsError::NotFound | FsError::NotADirectory) => None,
            Err(error) => return Err(error),
        };
        let at = n.unwrap_or(0);
        if at >= out.len() {
            return Ok(at);
        }
        match (n, self.store.list(path, &mut out[at..])) {
            (_, Ok(m)) => Ok(at + m),
            (Some(n), Err(FsError::NotFound | FsError::NotADirectory | FsError::Unavailable)) => {
                Ok(n)
            }
            (_, Err(error)) => Err(error),
        }
    }

    /// VIFS1 entries, then cell-store entries whose name VIFS1 does not
    /// already list (VIFS1 wins on a collision, as it does for reads).
    fn list_page(&self, path: &str, start: u32, out: &mut [u8]) -> Result<ListPage, FsError> {
        let boot_dir = matches!(self.boot.stat(path)?, Some((_, true)));
        let store_dir = matches!(self.store_stat(path)?, Some((_, true)));
        if !boot_dir && !store_dir {
            return Err(FsError::NotFound);
        }
        let mut written = 0;
        let store_start = if start & STORE_PHASE == 0 {
            if boot_dir {
                let page = self.boot.list_page(path, start, out)?;
                if page.next.is_some() {
                    return Ok(page);
                }
                written = page.written;
            } else if start != 0 {
                return Err(FsError::InvalidPath);
            }
            0
        } else {
            start & !STORE_PHASE
        };
        if !store_dir {
            if store_start != 0 {
                return Err(FsError::InvalidPath);
            }
            return Ok(ListPage {
                written,
                next: None,
            });
        }
        let shadowed = if boot_dir {
            self.boot.names(path)?
        } else {
            Vec::new()
        };
        let page =
            self.store
                .list_page_filtered(path, store_start, &mut out[written..], &|name| {
                    shadowed.iter().any(|boot| boot.as_ref() == name)
                })?;
        Ok(ListPage {
            written: written + page.written,
            next: page.next.map(|next| next | STORE_PHASE),
        })
    }

    fn stat(&self, path: &str) -> Result<Option<(u64, bool)>, FsError> {
        match self.boot.stat(path) {
            Ok(Some(hit)) => Ok(Some(hit)),
            Ok(None) => self.store_stat(path),
            // Found in the cell-store is still an answer; absent is not.
            Err(error) => self.store_stat(path)?.map(Some).ok_or(error),
        }
    }

    fn file_size(&self, path: &str) -> Result<u64, FsError> {
        boot_first(
            self.boot.file_size(path),
            |s| *s == 0,
            || self.store.file_size(path),
        )
    }

    fn read_to_vec(&self, path: &str) -> Result<Vec<u8>, FsError> {
        boot_first(self.boot.read_to_vec(path), Vec::is_empty, || {
            self.store.read_to_vec(path)
        })
    }

    /// Positional read: VIFS1 answers first, then the on-disk cell-store — the same precedence
    /// `read_to_vec` uses, so the overlay never serves a cell-store copy of a bootstrap file.
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let boot = self.boot.read_at(path, offset, buf);
        boot_first(boot, |n| *n == 0, || self.store.read_at(path, offset, buf))
    }

    fn write(&mut self, _path: &str, _content: &[u8]) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn append(&mut self, _path: &str, _content: &[u8]) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn mkdir(&mut self, _path: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn rmdir(&mut self, _path: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn unlink(&mut self, _path: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn rmdir_recursive(&mut self, _path: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
}
