//! IPC request dispatcher — translates `DrvRequest` messages into VirtIO-blk I/O.
//!
//! Wire format: `types::block_wire`, shared with the AHCI/NVMe cells so VFS
//! speaks one block protocol.

use crate::device::BlkDevice;
use types::block_wire::{self, BlockOutcome, SECTOR_SIZE};

pub const REPLY_SIZE: usize = block_wire::REPLY_SIZE;

#[derive(PartialEq, Eq)]
enum DrvOp {
    Read,
    Write,
    Flush,
}

fn parse_op(data: &[u8]) -> Option<(DrvOp, u64)> {
    if data.len() < 2 {
        return None;
    }
    let op = match u16::from_le_bytes([data[0], data[1]]) {
        block_wire::OP_READ => DrvOp::Read,
        block_wire::OP_WRITE => DrvOp::Write,
        block_wire::OP_FLUSH => return Some((DrvOp::Flush, 0)),
        _ => return None,
    };
    if data.len() < 10 {
        return None;
    }
    let sector = u64::from_le_bytes(data[2..10].try_into().ok()?);
    Some((op, sector))
}

fn status(out: &mut [u8; REPLY_SIZE], outcome: BlockOutcome) -> usize {
    out[0] = outcome.to_byte();
    1
}

/// Handle one incoming IPC message. Writes the reply into `out` and returns the
/// number of bytes to send back: 513 for a completed read, else 1.
pub fn handle(dev: &mut BlkDevice, data: &[u8], out: &mut [u8; REPLY_SIZE]) -> usize {
    let Some((op, sector)) = parse_op(data) else {
        return status(out, BlockOutcome::NotIssued);
    };

    match op {
        DrvOp::Read => {
            let mut sec = [0u8; SECTOR_SIZE];
            match dev.read_sector(sector, &mut sec) {
                Ok(()) => {
                    out[0] = BlockOutcome::Completed.to_byte();
                    out[1..].copy_from_slice(&sec);
                    REPLY_SIZE
                }
                Err(outcome) => status(out, outcome),
            }
        }
        DrvOp::Write => {
            if data.len() < 10 + SECTOR_SIZE {
                return status(out, BlockOutcome::NotIssued);
            }
            let mut sec = [0u8; SECTOR_SIZE];
            sec.copy_from_slice(&data[10..10 + SECTOR_SIZE]);
            match dev.write_sector(sector, &sec) {
                Ok(()) => status(out, BlockOutcome::Completed),
                Err(outcome) => status(out, outcome),
            }
        }
        DrvOp::Flush => match dev.flush() {
            Ok(()) => status(out, BlockOutcome::Completed),
            Err(outcome) => status(out, outcome),
        },
    }
}
