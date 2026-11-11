// SPDX-License-Identifier: MPL-2.0
//! The snapshot API on kernels built without `snapshot-qualified`
//! (architecture remediation Phase 09): the 5.7k-line engine in
//! `snapshot_engine.rs` is not compiled at all, and this module is the only
//! closed-gate behaviour.
//!
//! - no scratch region is reserved at boot;
//! - a capture is refused with [`SnapshotError::GateClosed`];
//! - a boot never restores, and clears a header an older image may have left
//!   behind, so that image cannot replay it after a downgrade.

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

/// Cold boot only: clears a stale snapshot header and never restores.
pub fn try_restore() -> bool {
    if clear_stale_header(&mut KernelBlock) {
        log::info!("[snapshot] snapshot invalidated");
        log::warn!(
            "[snapshot] stale header invalidated: capture/restore disabled by the phase-01 \
             qualification gate (feature `snapshot-qualified`)"
        );
    }
    false
}

/// The sector operations `clear_stale_header` needs.
trait Sectors {
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> bool;
    fn write(&mut self, lba: u64, buf: &[u8]) -> bool;
    fn flush(&mut self) -> bool;
}

/// The kernel's block route.
struct KernelBlock;

impl Sectors for KernelBlock {
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> bool {
        crate::dev::block::read_sector(lba, buf).is_ok()
    }
    fn write(&mut self, lba: u64, buf: &[u8]) -> bool {
        crate::dev::block::write_sector(lba, buf).is_ok()
    }
    fn flush(&mut self) -> bool {
        crate::dev::block::flush().is_ok()
    }
}

/// Reads the header sector and zeroes it (then flushes) only when it carries
/// the snapshot magic, so a board without a snapshot region sees no stray
/// write. Returns whether a header was found and cleared; write and flush
/// failures are non-fatal because nothing on this kernel reads the region.
fn clear_stale_header(sectors: &mut impl Sectors) -> bool {
    let mut sector = [0u8; SECTOR_SIZE];
    if !sectors.read(SNAPSHOT_BASE_LBA, &mut sector) {
        return false;
    }
    let magic = u32::from_le_bytes([sector[0], sector[1], sector[2], sector[3]]);
    if magic != SNAPSHOT_MAGIC {
        return false;
    }
    let _ = sectors.write(SNAPSHOT_BASE_LBA, &[0u8; SECTOR_SIZE]);
    let _ = sectors.flush();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[derive(Default)]
    struct Fake {
        header: Option<[u8; SECTOR_SIZE]>,
        writes: Vec<(u64, [u8; SECTOR_SIZE])>,
        flushes: usize,
    }

    impl Sectors for Fake {
        fn read(&mut self, lba: u64, buf: &mut [u8]) -> bool {
            assert_eq!(lba, SNAPSHOT_BASE_LBA);
            match self.header {
                Some(header) => {
                    buf.copy_from_slice(&header);
                    true
                }
                None => false,
            }
        }
        fn write(&mut self, lba: u64, buf: &[u8]) -> bool {
            self.writes.push((lba, buf.try_into().unwrap()));
            true
        }
        fn flush(&mut self) -> bool {
            self.flushes += 1;
            true
        }
    }

    fn header_with_magic(magic: u32) -> [u8; SECTOR_SIZE] {
        let mut sector = [0xA5u8; SECTOR_SIZE];
        sector[..4].copy_from_slice(&magic.to_le_bytes());
        sector
    }

    #[test]
    fn gate_closed_capture_is_refused() {
        assert_eq!(serialize_snapshot(), Err(SnapshotError::GateClosed));
    }

    #[test]
    fn unreadable_region_is_left_alone() {
        let mut fake = Fake::default();
        assert!(!clear_stale_header(&mut fake));
        assert!(fake.writes.is_empty());
        assert_eq!(fake.flushes, 0);
    }

    #[test]
    fn foreign_sector_is_not_written() {
        let mut fake = Fake {
            header: Some(header_with_magic(SNAPSHOT_MAGIC ^ 1)),
            ..Fake::default()
        };
        assert!(!clear_stale_header(&mut fake));
        assert!(fake.writes.is_empty());
        assert_eq!(fake.flushes, 0);
    }

    #[test]
    fn stale_header_is_zeroed_and_flushed_once() {
        let mut fake = Fake {
            header: Some(header_with_magic(SNAPSHOT_MAGIC)),
            ..Fake::default()
        };
        assert!(clear_stale_header(&mut fake));
        assert_eq!(fake.writes.len(), 1);
        assert_eq!(fake.writes[0].0, SNAPSHOT_BASE_LBA);
        assert!(fake.writes[0].1.iter().all(|byte| *byte == 0));
        assert_eq!(fake.flushes, 1);
    }
}
