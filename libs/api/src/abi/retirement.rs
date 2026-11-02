// SPDX-License-Identifier: Apache-2.0
//! Cell incarnation retirement verdict ABI (`QueryRetirement = 431`).
//! The kernel writes one [`ViRetirementV1`] per retired incarnation root, after
//! quiescence (`NotifyOnExit` still fires earlier, at `exit_task`). Class K record
//! (`docs/abi-boundary-records.md`): syscall 431 plus exact length, version exactly
//! 1, reserved fields zero, undefined `state` values and `reasons` bits refused,
//! little-endian in declared field order. No record (unknown or evicted root) must
//! be treated as [`RetirementState::Unproven`].

/// Exact byte length of [`ViRetirementV1`].
pub const RETIREMENT_RECORD_LEN: usize = 96;
/// The only record version this layout writes and accepts.
pub const RETIREMENT_RECORD_VERSION: u16 = 1;

/// Verdict for one retired incarnation.
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetirementState {
    /// Quiescence or deferred DMA is not complete yet.
    Pending = 1,
    /// No outstanding obligation remains.
    Contained = 2,
    /// Only counted, bounded quarantines remain (pins, VFS leases, deferred DMA).
    Quarantined = 3,
    /// Memory reachable by live peers cannot be proven released; see the reasons.
    Unproven = 4,
}

/// [`ViRetirementV1::reasons`] bits. Every bit outside [`reason::KNOWN`] is refused.
pub mod reason {
    /// The incarnation owned SHM frames: USER mappings in the shared root with no
    /// borrower record. Forces `Unproven`.
    pub const SHM_UNTRACKED: u32 = 1 << 0;
    /// The bounded pin quarantine overflowed and pages were leaked. Forces `Unproven`.
    pub const QUARANTINE_OVERFLOW: u32 = 1 << 1;
    /// The incarnation died holding writable grantee references to a live peer's
    /// grant. Reported only, never classified: the grant owner must treat that
    /// memory's invariants as unproven and never unlock or reuse a lock inside it.
    pub const WRITABLE_BORROW: u32 = 1 << 2;
    /// IOMMU/DMA release was deferred and is retried. Gives `Quarantined`.
    pub const DMA_DEFERRED: u32 = 1 << 3;
    /// Pins, VFS leases or quarantined pages remain bounded. Gives `Quarantined`.
    pub const TRACKED_QUARANTINE: u32 = 1 << 4;
    /// Every defined bit.
    pub const KNOWN: u32 = 0x1F;
}

/// Retirement record, version 1. Field order is the wire order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViRetirementV1 {
    /// [`RETIREMENT_RECORD_VERSION`].
    pub version: u16,
    /// [`RetirementState`] discriminant.
    pub state: u16,
    /// [`reason`] bits.
    pub reasons: u32,
    /// Tasks of the incarnation reaped by the retirement funnel, root included.
    pub member_threads: u32,
    /// Zero.
    pub reserved0: u32,
    /// Incarnation Cell id.
    pub cell_id: u64,
    /// Incarnation generation.
    pub generation: u64,
    /// Root task of the incarnation; the query key.
    pub root_tid: u64,
    /// Bit `h` set: retirement awaited a remote switch completion on hart `h`.
    pub remote_hart_mask: u64,
    /// Grants owned by the incarnation that were reclaimed.
    pub grants_reclaimed: u32,
    /// Grantee references to other owners' grants that were cleared.
    pub grantee_refs_cleared: u32,
    /// Cleared grantee references that were writable.
    pub writable_borrows: u32,
    /// Pin regions moved to quarantine.
    pub pins_quarantined: u32,
    /// VFS leases granted by the incarnation still awaiting holder release.
    pub vfs_leases_pending: u32,
    /// SHM frames the incarnation owned that were released.
    pub shm_frames_released: u32,
    /// Members whose DMA release was deferred.
    pub dma_deferred: u32,
    /// Zero.
    pub reserved1: u32,
    /// Quarantined pages attributed to the incarnation's members.
    pub quarantined_pages: u64,
    /// Pages leaked during the retirement window. May over-report, never under.
    pub leaked_pages: u64,
}

const _: () = assert!(core::mem::size_of::<ViRetirementV1>() == RETIREMENT_RECORD_LEN);

/// Why bytes are not a valid V1 record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordError {
    /// Length is not exactly [`RETIREMENT_RECORD_LEN`].
    Length,
    /// `version` is not [`RETIREMENT_RECORD_VERSION`].
    UnsupportedVersion,
    /// A reserved field is non-zero.
    Reserved,
    /// `state` is not a defined [`RetirementState`].
    UnknownState,
    /// `reasons` has a bit outside [`reason::KNOWN`].
    UnknownReason,
}

impl ViRetirementV1 {
    /// Little-endian wire bytes.
    pub fn encode(&self) -> [u8; RETIREMENT_RECORD_LEN] {
        let mut out = [0u8; RETIREMENT_RECORD_LEN];
        out[0..2].copy_from_slice(&self.version.to_le_bytes());
        out[2..4].copy_from_slice(&self.state.to_le_bytes());
        let head = [self.reasons, self.member_threads, self.reserved0];
        for (i, v) in head.iter().enumerate() {
            out[4 + i * 4..8 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        let ids = [
            self.cell_id,
            self.generation,
            self.root_tid,
            self.remote_hart_mask,
        ];
        for (i, v) in ids.iter().enumerate() {
            out[16 + i * 8..24 + i * 8].copy_from_slice(&v.to_le_bytes());
        }
        let counts = [
            self.grants_reclaimed,
            self.grantee_refs_cleared,
            self.writable_borrows,
            self.pins_quarantined,
            self.vfs_leases_pending,
            self.shm_frames_released,
            self.dma_deferred,
            self.reserved1,
        ];
        for (i, v) in counts.iter().enumerate() {
            out[48 + i * 4..52 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        out[80..88].copy_from_slice(&self.quarantined_pages.to_le_bytes());
        out[88..96].copy_from_slice(&self.leaked_pages.to_le_bytes());
        out
    }

    /// Decode and fully validate wire bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, RecordError> {
        if bytes.len() != RETIREMENT_RECORD_LEN {
            return Err(RecordError::Length);
        }
        let le = |at: usize, n: usize| {
            bytes[at..at + n]
                .iter()
                .rev()
                .fold(0u64, |acc, b| acc << 8 | u64::from(*b))
        };
        let record = Self {
            version: le(0, 2) as u16,
            state: le(2, 2) as u16,
            reasons: le(4, 4) as u32,
            member_threads: le(8, 4) as u32,
            reserved0: le(12, 4) as u32,
            cell_id: le(16, 8),
            generation: le(24, 8),
            root_tid: le(32, 8),
            remote_hart_mask: le(40, 8),
            grants_reclaimed: le(48, 4) as u32,
            grantee_refs_cleared: le(52, 4) as u32,
            writable_borrows: le(56, 4) as u32,
            pins_quarantined: le(60, 4) as u32,
            vfs_leases_pending: le(64, 4) as u32,
            shm_frames_released: le(68, 4) as u32,
            dma_deferred: le(72, 4) as u32,
            reserved1: le(76, 4) as u32,
            quarantined_pages: le(80, 8),
            leaked_pages: le(88, 8),
        };
        if record.version != RETIREMENT_RECORD_VERSION {
            return Err(RecordError::UnsupportedVersion);
        }
        if record.reserved0 != 0 || record.reserved1 != 0 {
            return Err(RecordError::Reserved);
        }
        if record.retirement_state().is_none() {
            return Err(RecordError::UnknownState);
        }
        if record.reasons & !reason::KNOWN != 0 {
            return Err(RecordError::UnknownReason);
        }
        Ok(record)
    }

    /// The typed state; `None` for an undefined value.
    pub fn retirement_state(&self) -> Option<RetirementState> {
        match self.state {
            1 => Some(RetirementState::Pending),
            2 => Some(RetirementState::Contained),
            3 => Some(RetirementState::Quarantined),
            4 => Some(RetirementState::Unproven),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "retirement_tests.rs"]
mod tests;
