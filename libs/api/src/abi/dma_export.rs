// SPDX-License-Identifier: Apache-2.0
//! Client-memory DMA export and mapping records (`ExportDmaRange = 437`,
//! `MapDmaExport = 438`, `RevokeDmaExport = 439`); ADR-0033, CP1 §5.
//!
//! [`ViDmaExportV1`] is a class K record (`docs/abi-boundary-records.md`):
//! syscall 438 with an exact out length of [`DMA_EXPORT_RECORD_LEN`] (48 bytes),
//! version exactly 1, little-endian in declared field order, every reserved byte
//! and unknown flag bit zero. Producer: the kernel. Consumer: the Driver Cell
//! that holds the lease and owns the BDF.
//!
//! Every refusal of 437-439 returns one of the `ERR_*` sentinels below; a
//! successful 437 returns an export id in `1..=isize::MAX`, which never
//! collides with them.

use types::ViError;

/// The only record version 438 writes and a v1 consumer accepts.
pub const DMA_EXPORT_VERSION: u16 = 1;
/// Exact `out_len` syscall 438 accepts and bytes it writes.
pub const DMA_EXPORT_RECORD_LEN: usize = 48;

/// Maximum bytes a single export may cover: 4 GiB (2^32).
pub const EXPORT_MAX_BYTES: u64 = 1 << 32;

/// `flags` bit: device is granted write access to the exported memory.
/// If clear, the export is device-read-only (`R`-only VT-d leaves).
pub const EXPORT_DEVICE_WRITE: u16 = 1 << 0;

/// All known flag bits in version 1.
pub const EXPORT_FLAGS_KNOWN: u16 = EXPORT_DEVICE_WRITE;

/// Holder not eligible, range not owned, caller not the lease holder, or
/// 438 without allowlist bit 48 / `PcieDriverCap` / BDF ownership.
pub const ERR_PERMISSION_DENIED: usize = usize::MAX;
/// 439: the window invalidation was not acknowledged; the lease stays
/// `Revoking` with the owner range locked and the kernel retries.
pub const ERR_TRY_AGAIN: usize = usize::MAX - 1;
/// No live provider (437), or an unknown, revoked or owner-dead export (438),
/// or a caller that is neither owner nor holder (439, no existence oracle).
pub const ERR_FILE_NOT_FOUND: usize = usize::MAX - 2;
/// Malformed arguments: alignment, length, flags, service or BDF bits,
/// `out_len != 48`, non-zero reserved registers.
pub const ERR_INVALID_INPUT: usize = usize::MAX - 3;
/// Reserved for the record family; 438 refuses a short buffer as invalid input.
pub const ERR_BUFFER_TOO_SMALL: usize = usize::MAX - 4;
/// No translating remapper for the export or the BDF: exports never fall back
/// to identity DMA.
pub const ERR_NOT_SUPPORTED: usize = usize::MAX - 5;
/// Lease table, IOVA window space, page tables or the holder's DMA quota.
pub const ERR_OUT_OF_MEMORY: usize = usize::MAX - 6;
/// 438: the window was published but its invalidation was not acknowledged;
/// only revoke or death cleans it.
pub const ERR_UNKNOWN: usize = usize::MAX - 7;

/// Export record returned by `MapDmaExport` (syscall 438).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ViDmaExportV1 {
    /// Exactly [`DMA_EXPORT_VERSION`].
    pub version: u16,
    /// Echo of the export flags ([`EXPORT_DEVICE_WRITE`]).
    pub flags: u16,
    /// Zero.
    pub reserved0: u32,
    /// Echo of the export id.
    pub export_id: u64,
    /// The exporting Cell. The driver must match it against the caller identity
    /// of the request that carried the id (confused-deputy check).
    pub owner_cell_id: u64,
    /// Generation of the exporting Cell.
    pub owner_generation: u64,
    /// Device-visible window base in the holder's domain, 4 KiB aligned.
    pub iova: u64,
    /// Bytes covered, echo of the export length.
    pub len: u64,
}

impl ViDmaExportV1 {
    /// A version-1 record with zero reserved bytes.
    pub const fn new(
        flags: u16,
        export_id: u64,
        owner_cell_id: u64,
        owner_generation: u64,
        iova: u64,
        len: u64,
    ) -> Self {
        Self {
            version: DMA_EXPORT_VERSION,
            flags,
            reserved0: 0,
            export_id,
            owner_cell_id,
            owner_generation,
            iova,
            len,
        }
    }

    /// Serialize in declared field order, little-endian.
    pub fn encode(&self, out: &mut [u8; DMA_EXPORT_RECORD_LEN]) {
        out[0..2].copy_from_slice(&self.version.to_le_bytes());
        out[2..4].copy_from_slice(&self.flags.to_le_bytes());
        out[4..8].copy_from_slice(&self.reserved0.to_le_bytes());
        out[8..16].copy_from_slice(&self.export_id.to_le_bytes());
        out[16..24].copy_from_slice(&self.owner_cell_id.to_le_bytes());
        out[24..32].copy_from_slice(&self.owner_generation.to_le_bytes());
        out[32..40].copy_from_slice(&self.iova.to_le_bytes());
        out[40..48].copy_from_slice(&self.len.to_le_bytes());
    }

    /// Decode exactly [`DMA_EXPORT_RECORD_LEN`] bytes, refusing any other
    /// length, a version other than 1, unknown flag bits or non-zero reserved
    /// bytes with [`ViError::InvalidInput`].
    pub fn decode(buf: &[u8]) -> Result<Self, ViError> {
        let buf: &[u8; DMA_EXPORT_RECORD_LEN] =
            buf.try_into().map_err(|_| ViError::InvalidInput)?;
        let u64_at = |at: usize| {
            let mut word = [0u8; 8];
            word.copy_from_slice(&buf[at..at + 8]);
            u64::from_le_bytes(word)
        };
        let version = u16::from_le_bytes([buf[0], buf[1]]);
        let flags = u16::from_le_bytes([buf[2], buf[3]]);
        let reserved0 = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        if version != DMA_EXPORT_VERSION || flags & !EXPORT_FLAGS_KNOWN != 0 || reserved0 != 0 {
            return Err(ViError::InvalidInput);
        }
        Ok(Self {
            version,
            flags,
            reserved0,
            export_id: u64_at(8),
            owner_cell_id: u64_at(16),
            owner_generation: u64_at(24),
            iova: u64_at(32),
            len: u64_at(40),
        })
    }
}
