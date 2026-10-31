//! IPC request dispatcher — translates `DrvRequest` messages into NVMe I/O.
//!
//! Wire format: `types::block_wire`, shared with the AHCI/virtio-blk cells.

use crate::controller::NvmeController;
use crate::dma::AuthorizedDma;
use ostd::dma::DmaBuf;
use types::block_wire::{self, BlockOutcome, SECTOR_SIZE};

pub const REPLY_SIZE: usize = block_wire::REPLY_SIZE;

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

/// Handle one incoming IPC message. Writes the reply into `out` and returns
/// the number of bytes to send back: 513 for a completed read, else 1.
pub fn handle(
    ctrl: &mut NvmeController,
    io_buf: &AuthorizedDma<DmaBuf>,
    data: &[u8],
    out: &mut [u8; REPLY_SIZE],
) -> usize {
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

    match op {
        DrvOp::Read => match ctrl.read_sector(sector, io_buf.iova()) {
            Ok(()) => {
                out[0] = BlockOutcome::Completed.to_byte();
                // SAFETY: read_sector completed; DMA data is now stable in
                // the CPU mapping retained by io_buf.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        io_buf.inner().virt(),
                        out[1..].as_mut_ptr(),
                        SECTOR_SIZE,
                    );
                }
                REPLY_SIZE
            }
            Err(outcome) => status(out, outcome),
        },

        DrvOp::Write => {
            if data.len() < 10 + SECTOR_SIZE {
                return status(out, BlockOutcome::NotIssued);
            }
            // SAFETY: write the caller payload into the CPU mapping before
            // submitting the authorized device-visible IOVA.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    data[10..].as_ptr(),
                    io_buf.inner().virt(),
                    SECTOR_SIZE,
                );
            }
            match ctrl.write_sector(sector, io_buf.iova()) {
                Ok(()) => status(out, BlockOutcome::Completed),
                Err(outcome) => status(out, outcome),
            }
        }
        DrvOp::Flush => match ctrl.flush() {
            Ok(()) => status(out, BlockOutcome::Completed),
            Err(outcome) => status(out, outcome),
        },
    }
}
