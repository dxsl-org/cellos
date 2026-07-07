//! Boot-time cell loader — reads cell ELFs directly from the block device.
//!
//! Used during early boot before the VFS Cell is running.  Reads the cell
//! bootstrap section appended to `disk_v3.img` at `CELL_TABLE_BASE_LBA`.
//!
//! Call sequence:
//! 1. `EarlyLoader::probe()` — reads and validates the cell table header.
//! 2. `EarlyLoader::read_file(path)` — returns an owned `Box<[u8]>` of the ELF.

use super::disk_layout::{
    CellEntry, CellTableHeader, CELL_PATH_LEN, CELL_TABLE_BASE_LBA, CELL_TABLE_MAGIC,
    MAX_CELL_ENTRIES, SECTOR_SIZE,
};
use alloc::boxed::Box;
use alloc::vec::Vec;
use types::{ViError, ViResult};

/// Cached cell table loaded from disk at boot.
///
/// `None` until `EarlyLoader::probe()` is called successfully.
static CELL_TABLE: crate::sync::Spinlock<Option<EarlyTable>> =
    crate::sync::Spinlock::new(None);

struct EarlyTable {
    entries: Vec<CellEntry>,
}

/// Boot-time cell loader backed by the VirtIO block driver.
pub struct EarlyLoader;

/// Bootstrap cells embedded in the VIFS1 ramdisk (`kernel_fs.img`). These resolve
/// from RAM before the block device, so the boot path needs no block driver — the
/// foundation of the G2 loader redesign (ramdisk boot). `/bin/block` (the
/// virtio-blk Driver Cell) joins this list once it lands (plan phase 02); `init`
/// is embedded separately via `include_bytes!` and never goes through `read_file`.
pub const BOOTSTRAP_CELLS: &[&str] = &[
    "/bin/platform",
    "/bin/block",
    "/bin/vfs",
    "/bin/config",
    "/bin/shell",
];

/// True if `path` is a bootstrap cell that must resolve from the VIFS1 ramdisk
/// before the block device, so boot does not depend on a kernel block driver.
pub fn is_bootstrap_path(path: &str) -> bool {
    BOOTSTRAP_CELLS.contains(&path)
}

impl EarlyLoader {
    /// Read the cell bootstrap table from disk and cache it.
    ///
    /// Must be called after the VirtIO block driver is initialised but before
    /// any `read_file` call.  Idempotent — safe to call more than once.
    ///
    /// # Errors
    /// Returns `ViError::NotFound` if no block device is attached.
    /// Returns `ViError::InvalidInput` if the magic bytes do not match
    /// (disk image was not generated with `gen_disk.ps1`).
    pub fn probe() -> ViResult<()> {
        
        

        // Idempotent: skip if already probed.
        if CELL_TABLE.lock().is_some() {
            return Ok(());
        }

        // ── Read header sector ───────────────────────────────────────────────
        let mut header_buf = [0u8; SECTOR_SIZE];
        crate::task::drivers::block::read_sector(CELL_TABLE_BASE_LBA, &mut header_buf)?;

        // SAFETY: header_buf is SECTOR_SIZE bytes aligned to u8; CellTableHeader
        // is repr(C) and also SECTOR_SIZE bytes.  Transmute is safe here.
        let header: CellTableHeader = unsafe {
            core::mem::transmute(header_buf)
        };

        if header.magic != CELL_TABLE_MAGIC {
            log::warn!(
                "[early] cell table magic mismatch: got 0x{:016X}, want 0x{:016X}",
                header.magic,
                CELL_TABLE_MAGIC
            );
            return Err(ViError::InvalidInput);
        }

        let count = header.count as usize;
        if count > MAX_CELL_ENTRIES {
            log::error!("[early] cell table count {} exceeds MAX_CELL_ENTRIES", count);
            return Err(ViError::InvalidInput);
        }

        // ── Read entry sectors ───────────────────────────────────────────────
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let entry_lba = CELL_TABLE_BASE_LBA + 1 + i as u64;
            let mut entry_buf = [0u8; SECTOR_SIZE];
            crate::task::drivers::block::read_sector(entry_lba, &mut entry_buf)?;
            // SAFETY: entry_buf is SECTOR_SIZE bytes; CellEntry is repr(C) SECTOR_SIZE.
            let entry: CellEntry = unsafe { core::mem::transmute(entry_buf) };
            entries.push(entry);
        }

        log::info!("[early] cell table loaded: {} entries", count);
        for e in &entries {
            let path = core::str::from_utf8(&e.path[..CELL_PATH_LEN])
                .unwrap_or("?")
                .trim_end_matches('\0');
            log::debug!("[early]   {} @ LBA {} ({} bytes)", path, e.data_lba, e.data_size);
        }

        *CELL_TABLE.lock() = Some(EarlyTable { entries });
        Ok(())
    }

    /// Read a cell ELF from the block-device bootstrap table into a heap buffer.
    ///
    /// # Errors
    /// `ViError::NotFound` if the table is unprobed or lacks `path`;
    /// `ViError::InvalidInput` if the entry has zero size.
    fn read_from_block_table(path: &str) -> ViResult<Box<[u8]>> {
        let (data_lba, size) = {
            let guard = CELL_TABLE.lock();
            let table = guard.as_ref().ok_or(ViError::NotFound)?;
            let entry = table.entries.iter().find(|e| {
                let stored = core::str::from_utf8(&e.path[..CELL_PATH_LEN])
                    .unwrap_or("")
                    .trim_end_matches('\0');
                stored == path
            }).ok_or(ViError::NotFound)?;
            (entry.data_lba, entry.data_size as usize)
        };
        if size == 0 { return Err(ViError::InvalidInput); }
        let sector_count = (size + SECTOR_SIZE - 1) / SECTOR_SIZE;
        let mut buf = alloc::vec![0u8; sector_count * SECTOR_SIZE];
        for i in 0..sector_count {
            let lba = data_lba + i as u64;
            let offset = i * SECTOR_SIZE;
            crate::task::drivers::block::read_sector(lba, &mut buf[offset..offset + SECTOR_SIZE])?;
        }
        buf.truncate(size);
        Ok(buf.into_boxed_slice())
    }

    /// Read a cell ELF into a heap-allocated buffer.
    ///
    /// Resolution order depends on whether the path is a bootstrap cell:
    /// - **Bootstrap** ([`is_bootstrap_path`]): VIFS1 ramdisk (RAM) FIRST, block
    ///   table only as a transitional fallback. This is what lets the boot path
    ///   run with no block driver (G2 loader redesign).
    /// - **Non-bootstrap**: block table first, VIFS1 fallback (historical order,
    ///   unchanged until those cells migrate to a VFS-served store — plan phase 03).
    ///
    /// `path` must match what `gen_disk.ps1` wrote (e.g. `/bin/vfs`).
    ///
    /// # Errors
    /// Returns `ViError::NotFound` if neither source has the path.
    pub fn read_file(path: &str) -> ViResult<Box<[u8]>> {
        if is_bootstrap_path(path) {
            match crate::fs::read_file_from_vifs1(path) {
                Ok(buf) => {
                    // Runtime evidence (G2 loader redesign phase 01): bootstrap cell
                    // loaded from RAM, not the block device. info! now that the
                    // migration is verified across arches — one-time boot output,
                    // suppressed for vfs/config/shell (spawned by init after the log
                    // level drops to Warn, main.rs) during normal operation.
                    log::info!("[early] bootstrap {} <- VIFS1 ramdisk ({} bytes)", path, buf.len());
                    return Ok(buf);
                }
                Err(_) => {
                    log::warn!("[early] bootstrap {:?} not in VIFS1 — falling back to block table", path);
                    return Self::read_from_block_table(path);
                }
            }
        }

        match Self::read_from_block_table(path) {
            Ok(buf) => Ok(buf),
            Err(_) => {
                log::debug!("[early] block table miss for {:?} — trying VIFS1", path);
                crate::fs::read_file_from_vifs1(path)
            }
        }
    }
}
