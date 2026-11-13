// SPDX-License-Identifier: MPL-2.0
//! Block Driver Cell IPC contract, shared by every block driver (AHCI, NVMe,
//! virtio-blk) and by VFS's `blk_router`, the only client.
//!
//! Requests (little-endian):
//! - read  (10 B):  `[op=0 (2B)] [sector (8B)]`
//! - write (522 B): `[op=1 (2B)] [sector (8B)] [data (512B)]`
//! - flush (2 B):   `[op=2 (2B)]`
//! - read a run (16 B): `[op=3 (2B)] [count (2B)] [reserved=0 (4B)] [sector (8B)]`
//!   — see [`ReadRunRequest`].
//!
//! Reply byte 0 is a [`BlockOutcome`]. A completed `OP_READ` appends the
//! 512-byte sector (513 B); a completed `OP_READ_RUN` appends its `count`
//! sectors; every other reply is 1 byte.
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
/// Request opcode: read `count` consecutive sectors, returned inline.
pub const OP_READ_RUN: u16 = 3;
/// Most sectors one `OP_READ_RUN` may name: the reply (outcome byte plus the
/// sectors) must fit one 4096-byte IPC message (`wire::ipc::IPC_BUF_SIZE`).
pub const MAX_RUN_SECTORS: u16 = 7;

/// Sector size carried by one request.
pub const SECTOR_SIZE: usize = 512;
/// Reply to a one-sector `OP_READ`: outcome byte plus the sector.
pub const REPLY_SIZE: usize = 1 + SECTOR_SIZE;
/// Largest reply: a completed `OP_READ_RUN` of `MAX_RUN_SECTORS`.
pub const MAX_REPLY_SIZE: usize = 1 + MAX_RUN_SECTORS as usize * SECTOR_SIZE;
const _: () = assert!(MAX_REPLY_SIZE <= 4096);

/// `OP_READ_RUN`: read `count` consecutive sectors starting at `sector`; a
/// completed reply carries them inline after the outcome byte.
///
/// Inline because the VFS cannot take data back through a Grant: while it serves
/// a request the kernel resolves only Grants the requesting client owns, never
/// one the VFS allocated itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadRunRequest {
    pub sector: u64,
    pub count: u16,
}

impl ReadRunRequest {
    pub const LEN: usize = 16;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0..2].copy_from_slice(&OP_READ_RUN.to_le_bytes());
        out[2..4].copy_from_slice(&self.count.to_le_bytes());
        out[8..16].copy_from_slice(&self.sector.to_le_bytes());
        out
    }

    /// Decode a request; `None` for another opcode, a short frame, non-zero
    /// reserved bytes, or `count` outside `1..=MAX_RUN_SECTORS`.
    pub fn decode(frame: &[u8]) -> Option<Self> {
        let frame = frame.get(..Self::LEN)?;
        if u16::from_le_bytes([frame[0], frame[1]]) != OP_READ_RUN || frame[4..8] != [0; 4] {
            return None;
        }
        let count = u16::from_le_bytes([frame[2], frame[3]]);
        if !(1..=MAX_RUN_SECTORS).contains(&count) {
            return None;
        }
        Some(Self {
            count,
            sector: u64::from_le_bytes(frame[8..16].try_into().ok()?),
        })
    }

    /// Length of the completed reply: outcome byte plus the sectors.
    pub fn reply_len(&self) -> usize {
        1 + usize::from(self.count) * SECTOR_SIZE
    }
}

/// Serve an `OP_READ_RUN` with `read_run`, which fills the whole run (`count`
/// sectors from the first) in one device command. Writes the reply into `out`
/// (at least [`MAX_REPLY_SIZE`] bytes) and returns its length: the outcome byte
/// plus the sectors when the read completed, else its outcome alone. A run past
/// `sectors` (the device size) is `NotIssued` before any read.
pub fn serve_read_run(
    request: &ReadRunRequest,
    sectors: u64,
    out: &mut [u8],
    read_run: impl FnOnce(u64, &mut [u8]) -> Result<(), BlockOutcome>,
) -> usize {
    let in_range = request
        .sector
        .checked_add(u64::from(request.count))
        .is_some_and(|end| end <= sectors);
    let len = request.reply_len();
    if !in_range || out.len() < len {
        out[0] = BlockOutcome::NotIssued.to_byte();
        return 1;
    }
    match read_run(request.sector, &mut out[1..len]) {
        Ok(()) => {
            out[0] = BlockOutcome::Completed.to_byte();
            len
        }
        Err(outcome) => {
            out[0] = outcome.to_byte();
            1
        }
    }
}

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
    use super::{
        serve_read_run, BlockOutcome, ReadRunRequest, MAX_REPLY_SIZE, MAX_RUN_SECTORS, OP_READ,
    };

    #[test]
    fn read_run_requests_round_trip_and_bad_frames_are_refused() {
        let request = ReadRunRequest {
            sector: 0x1_0000_0002,
            count: MAX_RUN_SECTORS,
        };
        let frame = request.encode();
        assert_eq!(ReadRunRequest::decode(&frame), Some(request));
        assert_eq!(request.reply_len(), MAX_REPLY_SIZE);

        assert_eq!(ReadRunRequest::decode(&frame[..15]), None, "short frame");
        let mut reserved = frame;
        reserved[5] = 1;
        assert_eq!(ReadRunRequest::decode(&reserved), None, "reserved bytes");
        for count in [0, MAX_RUN_SECTORS + 1] {
            let mut bad = frame;
            bad[2..4].copy_from_slice(&count.to_le_bytes());
            assert_eq!(ReadRunRequest::decode(&bad), None, "count {count}");
        }
        let mut other_op = frame;
        other_op[0..2].copy_from_slice(&OP_READ.to_le_bytes());
        assert_eq!(ReadRunRequest::decode(&other_op), None, "another opcode");
    }

    #[test]
    fn a_served_run_is_all_sectors_or_the_failure_alone() {
        let request = ReadRunRequest {
            sector: 10,
            count: 3,
        };
        let mut out = [0u8; MAX_REPLY_SIZE];
        let len = serve_read_run(&request, 13, &mut out, |lba, run| {
            assert_eq!((lba, run.len()), (10, 3 * 512));
            run.fill(7);
            Ok(())
        });
        assert_eq!(len, 1 + 3 * 512);
        assert_eq!(out[0], BlockOutcome::Completed.to_byte());
        assert!(out[1..len].iter().all(|&b| b == 7));

        let mut reads = 0;
        let len = serve_read_run(&request, 12, &mut out, |_, _| {
            reads += 1;
            Ok(())
        });
        assert_eq!(
            (len, out[0], reads),
            (1, BlockOutcome::NotIssued.to_byte(), 0),
            "past the end"
        );

        let len = serve_read_run(&request, 13, &mut out, |_, _| {
            Err(BlockOutcome::Indeterminate)
        });
        assert_eq!((len, out[0]), (1, BlockOutcome::Indeterminate.to_byte()));
    }

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
