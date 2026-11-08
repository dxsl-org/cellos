// SPDX-License-Identifier: Apache-2.0
//! Client-memory DMA export and mapping records (`ExportDmaRange = 437`,
//! `MapDmaExport = 438`, `RevokeDmaExport = 439`); ADR-0033, CP1 §5.
//!
//! [`ViDmaExportV1`] is a class K record (`docs/abi-boundary-records.md`):
//! syscall 438 with an exact out length of [`DMA_EXPORT_RECORD_LEN`] (48 bytes),
//! version exactly 1, little-endian in declared field order, every reserved byte
//! and unknown flag bit zero. Producer: the kernel. Consumer: the Driver Cell
//! that holds the lease and owns the BDF.

use types::ViError;

/// The only record version 438 writes and a v1 consumer accepts.
pub const DMA_EXPORT_VERSION: u16 = 1;
/// Exact `out_len` syscall 438 accepts and bytes it writes.
pub const DMA_EXPORT_RECORD_LEN: usize = 48;

/// Maximum bytes a single export may cover: 4 GiB (2^32).
pub const EXPORT_MAX_BYTES: u64 = 1 << 32;

/// `flags` bit: device is granted write access to the exported memory.
/// If clear, the export is device-read-only.
pub const EXPORT_DEVICE_WRITE: u16 = 1 << 0;

/// All known flag bits in version 1.
pub const EXPORT_FLAGS_KNOWN: u16 = EXPORT_DEVICE_WRITE;

/// Error sentinels for 437, 438, 439.
pub const ERR_PERMISSION_DENIED: usize = usize::MAX;
pub const ERR_TRY_AGAIN: usize = usize::MAX - 1;
pub const ERR_FILE_NOT_FOUND: usize = usize::MAX - 2;
pub const ERR_INVALID_INPUT: usize = usize::MAX - 3;
pub const ERR_BUFFER_TOO_SMALL: usize = usize::MAX - 4;
pub const ERR_NOT_SUPPORTED: usize = usize::MAX - 5;
pub const ERR_OUT_OF_MEMORY: usize = usize::MAX - 6;
pub const ERR_UNKNOWN: usize = usize::MAX - 7;

/// Export record returned by `MapDmaExport` (syscall 438).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ViDmaExportV1 {
    pub version: u16,
    pub flags: u16,
    pub reserved0: u32,
    pub export_id: u64,
    pub owner_cell_id: u64,
    pub owner_generation: u64,
    pub iova: u64,
    pub len: u64,
}

impl ViDmaExportV1 {
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

    pub fn decode(buf: &[u8]) -> Result<Self, ViError> {
        if buf.len() != DMA_EXPORT_RECORD_LEN {
            return Err(ViError::InvalidInput);
        }
        let version = u16::from_le_bytes([buf[0], buf[1]]);
        if version != DMA_EXPORT_VERSION {
            return Err(ViError::InvalidInput);
        }
        let flags = u16::from_le_bytes([buf[2], buf[3]]);
        if (flags & !EXPORT_FLAGS_KNOWN) != 0 {
            return Err(ViError::InvalidInput);
        }
        let reserved0 = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        if reserved0 != 0 {
            return Err(ViError::InvalidInput);
        }
        let export_id = u64::from_le_bytes(buf[8..16].try_into().unwrap());
        let owner_cell_id = u64::from_le_bytes(buf[16..24].try_into().unwrap());
        let owner_generation = u64::from_le_bytes(buf[24..32].try_into().unwrap());
        let iova = u64::from_le_bytes(buf[32..40].try_into().unwrap());
        let len = u64::from_le_bytes(buf[40..48].try_into().unwrap());

        Ok(Self {
            version,
            flags,
            reserved0,
            export_id,
            owner_cell_id,
            owner_generation,
            iova,
            len,
        })
    }
}
