// SPDX-License-Identifier: MPL-2.0
//! On-disk identity of a snapshot header, shared by the snapshot engine
//! (`snapshot.rs`, compiled only with `snapshot-qualified`) and the
//! always-present cold-boot guard (`snapshot_disabled.rs`), so both recognise
//! the same header.

/// Magic bytes identifying a ViCell snapshot image (little-endian `VICU`).
pub const SNAPSHOT_MAGIC: u32 = 0x5543_4956; // 'U','C','I','V' as bytes on disk

/// Reserved LBA range for snapshot storage in `disk_v3.img` — MBR partition P3.
/// Sector 0 = header; sector 1+ = address inventory then frame payload.
pub const SNAPSHOT_BASE_LBA: u64 = crate::loader::disk_layout::PART_SNAPSHOT_BASE_LBA;

/// Sector size assumed by the format; the header occupies exactly one sector.
pub const SECTOR_SIZE: usize = 512;
