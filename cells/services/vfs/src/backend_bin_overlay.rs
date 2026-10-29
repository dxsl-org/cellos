//! `/bin` overlay backend — unions the kernel VIFS1 ramdisk (`BootFsProxy`) with
//! the on-disk FAT cell-store, so `/bin/<cell>` resolves whether the ELF is a
//! bootstrap cell embedded in `kernel_fs.img` OR a non-bootstrap cell migrated to
//! the disk cell-store (G2 loader redesign, phase 03).
//!
//! Read ops try VIFS1 first (bootstrap / trusted-core wins on name collision),
//! then fall back to the cell-store. `list` unions both. Strictly read-only:
//! `/bin` is never writable, so every mutating op returns `false`.

use alloc::vec::Vec;

use crate::backend::{FsBackend, ListPage};
use crate::backend_bootfs::BootFsProxy;
use crate::backend_fat::FatBackend;

/// Cursor bit marking a position in the cell-store half of a `/bin` listing.
const STORE_PHASE: u32 = 1 << 30;

pub struct BinOverlay {
    boot: BootFsProxy,
    store: FatBackend,
}

impl BinOverlay {
    /// `store_base_lba` = `api::disk::PART_CELLSTORE_BASE_LBA`. The cell-store FAT
    /// is mounted with prefix `/bin` so `/bin/<cell>` strips to `<cell>` at the FAT
    /// root — matching the layout gen_disk writes into the cell-store.
    pub fn new(store_base_lba: u64) -> Self {
        Self {
            boot: BootFsProxy::new(),
            store: FatBackend::mount("/bin", store_base_lba),
        }
    }
}

impl FsBackend for BinOverlay {
    fn get_file_ptr(&self, path: &str) -> Option<(usize, usize)> {
        self.boot
            .get_file_ptr(path)
            .or_else(|| self.store.get_file_ptr(path))
    }

    fn list(&self, path: &str, out: &mut [u8]) -> usize {
        // Union: VIFS1 entries first, then cell-store entries into the remainder.
        // Bootstrap and P2-only cell sets are disjoint by construction, so no
        // dedup is needed (a rare collision would list the name twice — benign).
        let n = self.boot.list(path, out);
        if n >= out.len() {
            return n;
        }
        n + self.store.list(path, &mut out[n..])
    }

    /// VIFS1 entries, then cell-store entries whose name VIFS1 does not
    /// already list (VIFS1 wins on a collision, as it does for reads).
    fn list_page(&self, path: &str, start: u32, out: &mut [u8]) -> Option<ListPage> {
        let boot_dir = matches!(self.boot.stat(path), Some((_, true)));
        let store_dir = matches!(self.store.stat(path), Some((_, true)));
        if !boot_dir && !store_dir {
            return None;
        }
        let mut written = 0;
        let store_start = if start & STORE_PHASE == 0 {
            if boot_dir {
                let page = self.boot.list_page(path, start, out)?;
                if page.next.is_some() {
                    return Some(page);
                }
                written = page.written;
            } else if start != 0 {
                return None;
            }
            0
        } else {
            start & !STORE_PHASE
        };
        if !store_dir {
            return (store_start == 0).then_some(ListPage { written, next: None });
        }
        let shadowed = if boot_dir { self.boot.names(path)? } else { Vec::new() };
        let page = self.store.list_page_filtered(path, store_start, &mut out[written..], &|name| {
            shadowed.iter().any(|boot| boot.as_ref() == name)
        })?;
        Some(ListPage {
            written: written + page.written,
            next: page.next.map(|next| next | STORE_PHASE),
        })
    }

    fn stat(&self, path: &str) -> Option<(u64, bool)> {
        self.boot.stat(path).or_else(|| self.store.stat(path))
    }

    fn file_size(&self, path: &str) -> u64 {
        let s = self.boot.file_size(path);
        if s > 0 {
            s
        } else {
            self.store.file_size(path)
        }
    }

    fn read_to_vec(&self, path: &str) -> Vec<u8> {
        let v = self.boot.read_to_vec(path);
        if !v.is_empty() {
            v
        } else {
            self.store.read_to_vec(path)
        }
    }

    /// Positional read: VIFS1 answers first, then the on-disk cell-store — the same precedence
    /// `read_to_vec` uses, so the overlay never serves a cell-store copy of a bootstrap file.
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> usize {
        let n = self.boot.read_at(path, offset, buf);
        if n > 0 {
            return n;
        }
        self.store.read_at(path, offset, buf)
    }

    fn write(&mut self, _path: &str, _content: &[u8]) -> bool {
        false
    }
    fn append(&mut self, _path: &str, _content: &[u8]) -> bool {
        false
    }
    fn mkdir(&mut self, _path: &str) -> bool {
        false
    }
    fn rmdir(&mut self, _path: &str) -> bool {
        false
    }
    fn unlink(&mut self, _path: &str) -> bool {
        false
    }
    fn rmdir_recursive(&mut self, _path: &str) -> bool {
        false
    }
}
