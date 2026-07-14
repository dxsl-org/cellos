//! Constants shared between the kernel early loader and the disk-image builder.
//!
//! Since Milestone 2.5 Phase 03, `disk_v3.img` carries a real MBR partition
//! table at LBA 0 (written by `tools/write-mbr.py`; gen_disk.ps1 builds it):
//!
//! ```text
//! P1  0x0C  LBA   2,048 + 524,288   FAT32 interop volume (VFS /data today, /mnt/sd later)
//! P2  0x7F  LBA 526,336 +  33,664   cell bootstrap table + raw ELF blobs
//! P3  0x7D  LBA 560,000 + 240,000   kernel heap snapshot (Phase 29)
//! P4  0x7E  LBA 800,000 + 131,072   littlefs /data (Milestone 2.5 Phase 04)
//! ```
//!
//! The constants below are the contract; the on-disk MBR is parsed at boot by
//! [`verify_mbr`] to detect drift between image and kernel (warn-only — the
//! constants stay authoritative so a blank/legacy image still boots).
//!
//! The kernel reads the cell table from `CELL_TABLE_BASE_LBA` onwards using
//! the VirtIO block driver directly, before any userspace VFS Cell is running.
//!
//! Layout of the cell bootstrap section (inside P2):
//!
//! ```text
//! LBA CELL_TABLE_BASE_LBA + 0   : CellTableHeader  (one sector = 512 bytes)
//! LBA CELL_TABLE_BASE_LBA + 1   : CellEntry[0..MAX_CELL_ENTRIES]
//!                                   (one sector per entry, padded to 512 bytes)
//! LBA CELL_TABLE_BASE_LBA + 1 + MAX_CELL_ENTRIES : raw ELF data, concatenated
//!                                   (each ELF starts at its entry's `data_lba`)
//! ```

// Partition constants live in `api::disk` (the Law-1 contract shared with
// cells and image tools); re-exported here so kernel code keeps short paths.
pub use api::disk::{
    PART_CELLSTORE_BASE_LBA, PART_CELLSTORE_SECTORS, PART_CELLTBL_SECTORS, PART_FAT32_BASE_LBA,
    PART_FAT32_SECTORS, PART_LFS_BASE_LBA, PART_LFS_SECTORS, PART_SNAPSHOT_BASE_LBA,
    PART_SNAPSHOT_SECTORS, PART_SRV_BASE_LBA, PART_SRV_SECTORS,
};

/// Sector offset (from LBA 0) where the cell bootstrap section (P2) begins.
/// Equals `PART_FAT32_BASE_LBA + PART_FAT32_SECTORS` — the value predates the
/// MBR and is kept identical so the early loader needs no migration.
pub const CELL_TABLE_BASE_LBA: u64 = api::disk::PART_CELLTBL_BASE_LBA;

/// Magic bytes at the start of `CellTableHeader`; identifies a valid table.
pub const CELL_TABLE_MAGIC: u64 = 0x5649_4F53_5F43_454C; // "ViCell_CEL" in ASCII

/// Maximum number of cells that can appear in the bootstrap table.
pub const MAX_CELL_ENTRIES: usize = 64;

/// Maximum path length (bytes) for a cell path in the bootstrap table.
pub const CELL_PATH_LEN: usize = 64;

/// Maximum path length accepted by the `SpawnFromPath` syscall.
/// Must be ≥ `CELL_PATH_LEN`; defines the trust-boundary validation limit.
pub const MAX_CELL_PATH: usize = 256;

/// Size of one disk sector in bytes.
pub const SECTOR_SIZE: usize = 512;

/// Header at `CELL_TABLE_BASE_LBA + 0`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CellTableHeader {
    /// Must equal `CELL_TABLE_MAGIC`; reject the table otherwise.
    pub magic: u64,
    /// Number of valid entries in the entry array that follows.
    pub count: u32,
    /// Reserved / zero-padded to fill the sector.
    pub _pad: [u8; 500],
}

/// One entry in the cell table; stored starting at `CELL_TABLE_BASE_LBA + 1`.
/// Each entry is padded to exactly `SECTOR_SIZE` bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CellEntry {
    /// Null-terminated path (e.g. `/bin/vfs\0`).
    pub path: [u8; CELL_PATH_LEN],
    /// First LBA of the ELF data.
    pub data_lba: u64,
    /// Size of the ELF data in bytes (not rounded to sectors).
    pub data_size: u64,
    /// Reserved. (512 − 64 − 8 − 8 = 432 bytes)
    pub _pad: [u8; 432],
}

// Compile-time size checks: each header/entry must fit in one sector.
const _: () = assert!(core::mem::size_of::<CellTableHeader>() == SECTOR_SIZE);
const _: () = assert!(core::mem::size_of::<CellEntry>() == SECTOR_SIZE);

/// Expected MBR partition map: `(slot, type, start_lba, sectors)`.
/// P5 (SRV/RedoxFS) is outside the legacy 4-partition MBR limit — verified
/// separately by the VFS service's partition constants.
const EXPECTED_PARTS: [(usize, u8, u64, u64); 4] = [
    (0, 0x0C, PART_FAT32_BASE_LBA, PART_FAT32_SECTORS),
    (1, 0x7F, CELL_TABLE_BASE_LBA, PART_CELLTBL_SECTORS),
    (2, 0x7D, PART_SNAPSHOT_BASE_LBA, PART_SNAPSHOT_SECTORS),
    (3, 0x7E, PART_LFS_BASE_LBA, PART_LFS_SECTORS),
];

/// Parse LBA 0 and compare the MBR against the compiled-in layout.
///
/// Warn-only by design: the constants above remain authoritative (a legacy or
/// blank image must still boot), but any drift between the image builder and
/// the kernel is surfaced at boot instead of as silent corruption later.
/// Called once from kernel init after the VirtIO block driver is up.
pub fn verify_mbr() {
    let mut sector = [0u8; SECTOR_SIZE];
    if crate::task::drivers::block::read_sector(0, &mut sector).is_err() {
        log::warn!("[mbr] LBA 0 unreadable — skipping partition verification");
        return;
    }
    if sector[510] != 0x55 || sector[511] != 0xAA {
        log::warn!("[mbr] no MBR signature — legacy whole-disk image (pre-P03 layout)");
        return;
    }
    for (slot, ptype, start, size) in EXPECTED_PARTS {
        let e = 446 + slot * 16;
        let found_type = sector[e + 4];
        let found_start =
            u32::from_le_bytes([sector[e + 8], sector[e + 9], sector[e + 10], sector[e + 11]])
                as u64;
        let found_size = u32::from_le_bytes([
            sector[e + 12],
            sector[e + 13],
            sector[e + 14],
            sector[e + 15],
        ]) as u64;
        if found_type != ptype || found_start != start || found_size != size {
            log::warn!(
                "[mbr] P{} mismatch: image type={:#04x} start={} size={} — kernel expects type={:#04x} start={} size={}",
                slot + 1, found_type, found_start, found_size, ptype, start, size
            );
        } else {
            log::info!(
                "[mbr] P{} ok: type={:#04x} start={} sectors={}",
                slot + 1,
                ptype,
                start,
                size
            );
        }
    }
}
