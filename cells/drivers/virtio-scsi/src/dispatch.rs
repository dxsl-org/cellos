//! IPC request dispatcher — translates `DrvRequest` messages into SCSI I/O
//! on the selected disk.
//!
//! Wire format: `types::block_wire`, shared with the virtio-blk/NVMe/AHCI
//! cells so VFS speaks one block protocol. The disk's logical block is
//! 512 bytes (checked at bring-up), so one wire sector is one LBA.

use crate::proto::VscsiError;
use crate::scan::Disk;
use scsi_block::ScsiError;
use types::block_wire::{self, BlockOutcome, SECTOR_SIZE};

pub const REPLY_SIZE: usize = block_wire::REPLY_SIZE;

#[derive(Debug, PartialEq, Eq)]
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

/// `NotIssued` when the device never saw the command (request rejected by
/// the disk layer, or the queue refused it); `Indeterminate` otherwise.
fn outcome(error: &ScsiError<VscsiError>) -> BlockOutcome {
    match error {
        ScsiError::Transport(error) if !error.issued() => BlockOutcome::NotIssued,
        ScsiError::Misaligned { .. }
        | ScsiError::OutOfRange { .. }
        | ScsiError::TooLarge { .. } => BlockOutcome::NotIssued,
        _ => BlockOutcome::Indeterminate,
    }
}

fn status(out: &mut [u8; REPLY_SIZE], outcome: BlockOutcome) -> usize {
    out[0] = outcome.to_byte();
    1
}

/// Handle one incoming IPC message. Writes the reply into `out` and returns
/// the number of bytes to send back: 513 for a completed read, else 1.
pub fn handle(disk: &mut Disk, data: &[u8], out: &mut [u8; REPLY_SIZE]) -> usize {
    let Some((op, sector)) = parse_op(data) else {
        return status(out, BlockOutcome::NotIssued);
    };
    let result = match op {
        DrvOp::Read => {
            let mut sec = [0u8; SECTOR_SIZE];
            disk.read(sector, &mut sec).map(|()| {
                out[1..].copy_from_slice(&sec);
                REPLY_SIZE
            })
        }
        DrvOp::Write => {
            let Some(sec) = data.get(10..10 + SECTOR_SIZE) else {
                return status(out, BlockOutcome::NotIssued);
            };
            disk.write(sector, sec).map(|()| 1)
        }
        DrvOp::Flush => disk.flush().map(|()| 1),
    };
    match result {
        Ok(len) => {
            out[0] = BlockOutcome::Completed.to_byte();
            len
        }
        Err(error) => {
            let _ = ostd::io::print_fmt(format_args!(
                "[virtio-scsi] {op:?} sector={sector} failed: {error:?}\n"
            ));
            status(out, outcome(&error))
        }
    }
}
