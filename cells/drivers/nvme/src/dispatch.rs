//! IPC request dispatcher — translates `DrvRequest` messages into NVMe I/O.
//!
//! Wire format: `types::block_wire`, shared with the AHCI/virtio-blk cells.

use crate::bounce::BounceBuffer;
use crate::controller::{IoFailure, NvmeController};
use types::block_wire::{self, BlockOutcome, ReadRunRequest, SECTOR_SIZE};

pub const REPLY_SIZE: usize = block_wire::MAX_REPLY_SIZE;

#[derive(Debug, PartialEq, Eq)]
pub enum DrvOp {
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

/// An issued command that failed: its outcome is unknown, and a timed-out one
/// still owns its buffer page, so that page is quarantined before any reuse.
fn failed(bounce: &mut BounceBuffer, failure: IoFailure, out: &mut [u8; REPLY_SIZE]) -> usize {
    if failure == IoFailure::TimedOut {
        bounce.quarantine();
    }
    status(out, BlockOutcome::Indeterminate)
}

/// `OP_READ_RUN`: one NVMe read of `count` blocks into the bounce page, copied
/// into the reply after the outcome byte. `count <= MAX_RUN_SECTORS` (7) keeps
/// the transfer inside the page, so PRP1 alone describes it.
fn read_run(
    ctrl: &mut NvmeController,
    bounce: &mut BounceBuffer,
    request: &ReadRunRequest,
    out: &mut [u8; REPLY_SIZE],
) -> usize {
    let in_range = request
        .sector
        .checked_add(u64::from(request.count))
        .is_some_and(|end| end <= ctrl.n_sectors);
    if !in_range {
        let _ = ostd::io::print_fmt(format_args!(
            "[nvme] out-of-range run: sector={} count={} namespace_sectors={}\n",
            request.sector, request.count, ctrl.n_sectors
        ));
        return status(out, BlockOutcome::NotIssued);
    }
    let Some((page, iova)) = bounce.page().map(|buf| (buf.inner().virt(), buf.iova())) else {
        return status(out, BlockOutcome::NotIssued);
    };
    if usize::from(request.count) > crate::layout::blocks_in_one_prp(iova) {
        return status(out, BlockOutcome::NotIssued);
    }
    if let Err(failure) = ctrl.read_sectors(request.sector, request.count, iova) {
        return failed(bounce, failure, out);
    }
    let len = request.reply_len();
    out[0] = BlockOutcome::Completed.to_byte();
    // SAFETY: the read completed, so the bounce page holds `len - 1` stable
    // bytes (`count` blocks, checked to fit the page above).
    unsafe {
        core::ptr::copy_nonoverlapping(page as *const u8, out[1..len].as_mut_ptr(), len - 1);
    }
    len
}

/// Handle one incoming IPC message. Writes the reply into `out` and returns
/// the number of bytes to send back: the outcome byte plus the sectors of a
/// completed read, else 1.
pub fn handle(
    ctrl: &mut NvmeController,
    bounce: &mut BounceBuffer,
    data: &[u8],
    out: &mut [u8; REPLY_SIZE],
) -> usize {
    if let Some(request) = ReadRunRequest::decode(data) {
        return read_run(ctrl, bounce, &request, out);
    }
    let Some((op, sector)) = parse_op(data) else {
        return status(out, BlockOutcome::NotIssued);
    };

    // A sector past the end of the namespace cannot be served, and the device
    // only answers it with an opaque status - which is how the Tier-3 lane's
    // persistence failures surface. Name the caller's sector and the namespace
    // size instead, so a failing boot says whether the request or the size was
    // wrong.
    if matches!(op, DrvOp::Read | DrvOp::Write) && sector >= ctrl.n_sectors {
        let _ = ostd::io::print_fmt(format_args!(
            "[nvme] out-of-range request: op={:?} sector={} namespace_sectors={}\n",
            op, sector, ctrl.n_sectors
        ));
    }

    if op == DrvOp::Flush {
        return match ctrl.flush() {
            Ok(()) => status(out, BlockOutcome::Completed),
            Err(failure) => failed(bounce, failure, out),
        };
    }
    // `None` after too many abandoned commands, or when no clean page could
    // replace the last one.
    let Some((page, iova)) = bounce.page().map(|buf| (buf.inner().virt(), buf.iova())) else {
        return status(out, BlockOutcome::NotIssued);
    };

    if op == DrvOp::Read {
        return match ctrl.read_sector(sector, iova) {
            Ok(()) => {
                out[0] = BlockOutcome::Completed.to_byte();
                // SAFETY: read_sector completed; DMA data is now stable in
                // the CPU mapping of the live bounce page.
                unsafe {
                    core::ptr::copy_nonoverlapping(page, out[1..].as_mut_ptr(), SECTOR_SIZE);
                }
                block_wire::REPLY_SIZE
            }
            Err(failure) => failed(bounce, failure, out),
        };
    }

    if data.len() < 10 + SECTOR_SIZE {
        return status(out, BlockOutcome::NotIssued);
    }
    // SAFETY: write the caller payload into the CPU mapping before
    // submitting the authorized device-visible IOVA.
    unsafe {
        core::ptr::copy_nonoverlapping(data[10..].as_ptr(), page, SECTOR_SIZE);
    }
    // Fault-injection witnesses only. `F08B-DIE` kills this cell before
    // the write is issued, with its controller still enabled;
    // `F09C-DIE-ISSUED` kills it right after the write's doorbell.
    #[cfg(feature = "fault-inject")]
    {
        let payload = &data[10..10 + SECTOR_SIZE];
        if payload.windows(8).any(|window| window == b"F08B-DIE") {
            let _ = ostd::io::print_fmt(format_args!(
                "[nvme] fault-inject: driver exiting at lba={}\n",
                sector
            ));
            ostd::syscall::sys_exit(70);
        }
        if payload
            .windows(15)
            .any(|window| window == b"F09C-DIE-ISSUED")
        {
            ctrl.exit_after_next_doorbell();
        }
    }
    match ctrl.write_sector(sector, iova) {
        Ok(()) => status(out, BlockOutcome::Completed),
        Err(failure) => failed(bounce, failure, out),
    }
}
