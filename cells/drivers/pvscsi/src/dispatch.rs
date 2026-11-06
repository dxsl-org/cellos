//! IPC request dispatcher — translates `DrvRequest` messages into SCSI I/O
//! on the selected disk.
//!
//! Wire format: `types::block_wire`, shared with the virtio-blk/NVMe/AHCI
//! cells so VFS speaks one block protocol. The disk's logical block is
//! 512 bytes (checked at bring-up), so one wire sector is one LBA.

use crate::error::PvscsiError;
use crate::scan::Disk;
use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::sync::Mutex;
use ostd::syscall::{sys_lookup_service, sys_send};
use scsi_block::ScsiError;
use types::block_wire::{self, BlockOutcome, SECTOR_SIZE};

pub const REPLY_SIZE: usize = block_wire::REPLY_SIZE;

/// Tid of the registered VFS once a request from it was accepted. Tids are
/// never reissued, so a cached match stays valid for the life of that VFS.
static VFS_TID: AtomicUsize = AtomicUsize::new(0);

/// Whether `sender_tid` is the registered VFS, the only accepted block client.
fn sender_is_vfs(sender_tid: usize) -> bool {
    let cached = VFS_TID.load(Ordering::Relaxed);
    if cached != 0 && cached == sender_tid {
        return true;
    }
    if sys_lookup_service(api::syscall::service::VFS) == Some(sender_tid) {
        VFS_TID.store(sender_tid, Ordering::Relaxed);
        return true;
    }
    false
}

/// Serve one raw `DrvRequest` from `sender_tid`. Any sender but the
/// registered VFS gets `NotIssued` without touching the device. The reply is
/// a blocking send: the VFS client waits in a blocking receive.
pub fn serve(state: &Mutex<Option<Disk>>, sender_tid: usize, data: &[u8]) {
    let mut reply = [0u8; REPLY_SIZE];
    let len = if !sender_is_vfs(sender_tid) {
        let _ = ostd::io::print_fmt(format_args!(
            "[pvscsi] refused block request from tid={sender_tid}: sender is not the registered VFS\n"
        ));
        status(&mut reply, BlockOutcome::NotIssued)
    } else if let Some(disk) = state.lock().as_mut() {
        handle(disk, data, &mut reply)
    } else {
        status(&mut reply, BlockOutcome::NotIssued)
    };
    let _ = sys_send(sender_tid, &reply[..len]);
}

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
fn outcome(error: &ScsiError<PvscsiError>) -> BlockOutcome {
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
                "[pvscsi] {op:?} sector={sector} failed: {error:?}\n"
            ));
            status(out, outcome(&error))
        }
    }
}
