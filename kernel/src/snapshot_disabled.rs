// SPDX-License-Identifier: MPL-2.0
//! The snapshot API on kernels built without `snapshot-qualified`
//! (architecture remediation Phase 09): the 5.7k-line engine in `snapshot.rs`
//! is not compiled at all, and this module keeps exactly the behaviour that
//! engine had with its qualification gate closed.
//!
//! - no scratch region is reserved at boot;
//! - a capture is refused with [`SnapshotError::GateClosed`];
//! - a boot never restores, and clears a header an older image may have left
//!   behind, so that image cannot replay it after a downgrade.

use crate::dev::block;
use crate::snapshot_format::{SECTOR_SIZE, SNAPSHOT_BASE_LBA, SNAPSHOT_MAGIC};

/// Why a capture was refused. Only the gate exists on this kernel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapshotError {
    /// `snapshot-qualified` is off — the phase-01 gate.
    GateClosed,
}

impl core::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("capture unqualified (phase-01 gate: feature `snapshot-qualified`)")
    }
}

/// No capture scratch region exists on this kernel.
pub fn reserve_scratch_region_at_boot() {}

/// Capture is not built into this kernel.
pub fn serialize_snapshot() -> Result<u32, SnapshotError> {
    Err(SnapshotError::GateClosed)
}

/// Cold boot only. Clears a snapshot header found in the reserved region:
/// reads the header sector first and writes only when it carries the snapshot
/// magic, so a board without a snapshot region sees no stray write. Failures
/// are non-fatal.
pub fn try_restore() -> bool {
    let mut sector = [0u8; SECTOR_SIZE];
    if block::read_sector(SNAPSHOT_BASE_LBA, &mut sector).is_err() {
        return false;
    }
    let magic = u32::from_le_bytes([sector[0], sector[1], sector[2], sector[3]]);
    if magic != SNAPSHOT_MAGIC {
        return false;
    }
    let _ = block::write_sector(SNAPSHOT_BASE_LBA, &[0u8; SECTOR_SIZE]);
    let _ = block::flush();
    log::info!("[snapshot] snapshot invalidated");
    log::warn!(
        "[snapshot] stale header invalidated: capture/restore disabled by the phase-01 \
         qualification gate (feature `snapshot-qualified`)"
    );
    false
}
