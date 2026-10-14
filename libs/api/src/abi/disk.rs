//! Canonical MBR partition layout for `disk_v3.img` — the contract shared by
//! the kernel (`loader/disk_layout.rs`), cells (VFS block stream, native
//! backend) and the image builders (`tools/write-mbr.py`, `gen_disk.ps1`,
//! `tools/mkfat32_inplace.py` — the Python tools carry a must-match copy).
//!
//! The on-disk MBR at LBA 0 is the runtime source of truth; the kernel parses
//! and cross-checks it at boot (`disk_layout::verify_mbr`, warn-only). These
//! constants stay authoritative so a blank or legacy image still boots, while
//! native open refuses unprovisioned/incompatible volumes without formatting.
//!
//! ```text
//! P1  0x0C  FAT32 interop volume      (VFS /mnt/sd)
//! P2  0x7F  cell bootstrap table      (kernel early loader ONLY — never granted)
//! P3  0x7D  kernel heap snapshot      (Phase 29 — kernel ONLY, never granted)
//! P4  0x7E  CellosFS Native /data      (offline provisioned; legacy LFS symbol names)
//! P5        CellosFS Native /srv      (constant-addressed, offline provisioned)
//! ```

/// P1: first absolute LBA of the FAT32 volume (MBR-standard 1 MiB alignment).
pub const PART_FAT32_BASE_LBA: u64 = 2_048;
/// P1: FAT32 volume size in sectors.
pub const PART_FAT32_SECTORS: u64 = 524_288;

/// P2: first LBA of the cell bootstrap table (header + entries + ELF blobs).
pub const PART_CELLTBL_BASE_LBA: u64 = 526_336;
/// P2 size in sectors (16 MiB).
pub const PART_CELLTBL_SECTORS: u64 = 33_664;

/// P3: kernel heap snapshot region (raw LBA, no filesystem).
pub const PART_SNAPSHOT_BASE_LBA: u64 = 560_000;
/// P3 size in sectors (~117 MB — covers the full 64 MB kernel heap + headers).
pub const PART_SNAPSHOT_SECTORS: u64 = 240_000;

/// P4: CellosFS Native /data store; legacy PART_LFS symbol names preserve ABI.
pub const PART_LFS_BASE_LBA: u64 = 800_000;
/// P4 size in sectors (64 MB).
pub const PART_LFS_SECTORS: u64 = 131_072;

/// P5: CellosFS Native filesystem — VFS `/srv` (G1+, ADR 0016).
///
/// Follows immediately after P4. Provisioned offline with the CellosFS library;
/// VFS opens existing volumes only and never formats blank/corrupt media.
/// Partition-capability gating (`MANIFEST_FLAG_PART_SRV`) requires expanding
/// `flags` to `u16` in a future manifest version; until then, `/srv` access is
/// controlled exclusively by the VFS `AccessTable`.
pub const PART_SRV_BASE_LBA: u64 = 931_072; // LBA 800_000 + 131_072
/// P5 size in sectors (64 MB CellosFS volume).
pub const PART_SRV_SECTORS: u64 = 131_072;

/// P6: FAT cell-store — non-bootstrap cell ELFs served under VFS `/bin`
/// (G2 loader redesign). Follows P5; constant-addressed (no MBR slot, like P5).
/// Read-only at runtime: the VFS `/bin` overlay reads it after a VIFS1 miss.
/// gen_disk formats + populates it with the signed P2-only cell ELFs.
pub const PART_CELLSTORE_BASE_LBA: u64 = 1_062_144; // LBA 931_072 + 131_072
/// P6 size in sectors (32 MB — ample for the P2-only cell set).
pub const PART_CELLSTORE_SECTORS: u64 = 65_536;
