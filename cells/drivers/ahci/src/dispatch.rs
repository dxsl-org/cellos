//! IPC request dispatcher — translates `DrvRequest` messages into AHCI I/O.
//!
//! Wire format: `types::block_wire`, shared with the NVMe/virtio-blk driver
//! cells so VFS's `blk_router` routes to any registered block driver unchanged.
//! Every sector is an absolute LBA on the whole device, which for a single
//! AHCI port is exactly the disk LBA space.

use crate::controller::AhciController;
use crate::dma::AuthorizedDma;
use ostd::dma::DmaBuf;
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

/// Handle one incoming IPC message. Writes the reply into `out` and returns
/// the number of bytes to send back: the outcome byte plus the sectors of a
/// completed read, else 1.
pub fn handle(
    ctrl: &mut AhciController,
    io_buf: &AuthorizedDma<DmaBuf>,
    data: &[u8],
    out: &mut [u8; REPLY_SIZE],
) -> usize {
    if let Some(request) = ReadRunRequest::decode(data) {
        let capacity = ctrl.capacity_sectors();
        return block_wire::serve_read_run(&request, capacity, out, |lba, sector| {
            ctrl.read_sector(lba, io_buf.iova())?;
            // SAFETY: read_sector completed; the DMA payload is now stable in
            // the CPU mapping retained by io_buf, which covers one page.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    io_buf.inner().virt(),
                    sector.as_mut_ptr(),
                    SECTOR_SIZE,
                );
            }
            Ok(())
        });
    }
    let Some((op, sector)) = parse_op(data) else {
        return status(out, BlockOutcome::NotIssued);
    };

    match op {
        DrvOp::Read => match ctrl.read_sector(sector, io_buf.iova()) {
            Ok(()) => {
                out[0] = BlockOutcome::Completed.to_byte();
                // SAFETY: read_sector completed; the DMA payload is now stable in
                // the CPU mapping retained by io_buf, which covers one page.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        io_buf.inner().virt(),
                        out[1..].as_mut_ptr(),
                        SECTOR_SIZE,
                    );
                }
                block_wire::REPLY_SIZE
            }
            Err(outcome) => status(out, outcome),
        },

        DrvOp::Write => {
            if data.len() < 10 + SECTOR_SIZE {
                return status(out, BlockOutcome::NotIssued);
            }
            let payload = &data[10..10 + SECTOR_SIZE];
            // SAFETY: copy the caller payload into the CPU mapping before
            // submitting the authorized device-visible IOVA.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    payload.as_ptr(),
                    io_buf.inner().virt(),
                    SECTOR_SIZE,
                );
            }
            #[cfg(feature = "fault-inject")]
            crate::fault_inject::before_write(ctrl, sector, payload);
            let outcome = match ctrl.write_sector(sector, io_buf.iova()) {
                Ok(()) => BlockOutcome::Completed,
                Err(outcome) => outcome,
            };
            #[cfg(feature = "fault-inject")]
            crate::fault_inject::after_write(sector, payload, outcome);
            status(out, outcome)
        }

        DrvOp::Flush => match ctrl.flush() {
            Ok(()) => status(out, BlockOutcome::Completed),
            Err(outcome) => status(out, outcome),
        },
    }
}
