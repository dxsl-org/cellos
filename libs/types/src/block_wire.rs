// SPDX-License-Identifier: MPL-2.0
//! Block Driver Cell IPC contract, shared by every block driver (AHCI, NVMe,
//! virtio-blk) and by VFS's `blk_router`, the only client.
//!
//! Requests (little-endian):
//! - read  (10 B):  `[op=0 (2B)] [sector (8B)]`
//! - write (522 B): `[op=1 (2B)] [sector (8B)] [data (512B)]`
//! - flush (2 B):   `[op=2 (2B)]`
//!
//! Reply byte 0 is a [`BlockOutcome`]. A completed read appends the 512-byte
//! sector (513 B); every other reply is 1 byte.
//!
//! The outcome reports **issue certainty**, not error cause. Once the device
//! has been notified (AHCI `PxCI`, NVMe SQ doorbell, virtio queue notify), a
//! timeout or a device-reported error does not undo the command: the medium may
//! already hold the new data, or part of it. Such a write or flush is
//! `Indeterminate` and must never be replayed automatically.

/// Request opcode: read one sector.
pub const OP_READ: u16 = 0;
/// Request opcode: write one sector.
pub const OP_WRITE: u16 = 1;
/// Request opcode: flush the device write cache.
pub const OP_FLUSH: u16 = 2;

/// Sector size carried by one request.
pub const SECTOR_SIZE: usize = 512;
/// Largest reply: outcome byte plus one sector.
pub const REPLY_SIZE: usize = 1 + SECTOR_SIZE;

/// Result of one block request, as seen by the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockOutcome {
    /// The device completed the command successfully.
    Completed,
    /// Refused before the device was notified (malformed request, LBA out of
    /// range, driver not initialised or poisoned, transport refused the send).
    /// Nothing reached the medium; the caller may retry.
    NotIssued,
    /// The device was, or may have been, notified and the command did not
    /// complete cleanly. Medium state is unknown; writes and flushes must not
    /// be replayed automatically.
    Indeterminate,
}

impl BlockOutcome {
    /// Wire byte for this outcome.
    pub const fn to_byte(self) -> u8 {
        match self {
            Self::Completed => 0x00,
            Self::NotIssued => 0x01,
            Self::Indeterminate => 0x02,
        }
    }

    /// Decode a reply byte. Any byte this contract does not define is
    /// `Indeterminate`: a reply that cannot be understood proves nothing
    /// about whether the device was notified.
    pub const fn from_byte(byte: u8) -> Self {
        match byte {
            0x00 => Self::Completed,
            0x01 => Self::NotIssued,
            _ => Self::Indeterminate,
        }
    }

    pub const fn is_completed(self) -> bool {
        matches!(self, Self::Completed)
    }
}

#[cfg(test)]
mod tests {
    use super::BlockOutcome;

    #[test]
    fn undefined_reply_bytes_decode_as_indeterminate() {
        for byte in 0x03..=0xFF {
            assert_eq!(BlockOutcome::from_byte(byte), BlockOutcome::Indeterminate);
        }
        for outcome in [
            BlockOutcome::Completed,
            BlockOutcome::NotIssued,
            BlockOutcome::Indeterminate,
        ] {
            assert_eq!(BlockOutcome::from_byte(outcome.to_byte()), outcome);
        }
    }
}
