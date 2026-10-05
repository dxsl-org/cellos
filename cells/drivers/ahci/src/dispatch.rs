//! IPC request dispatcher — translates `DrvRequest` messages into AHCI I/O.
//!
//! Wire format (little-endian), identical to the NVMe/virtio-blk driver cells
//! so VFS's `blk_router` routes to any registered block driver unchanged:
//!
//! Read request  (10 B): `[op=0 (2B)] [sector (8B)]`
//! Write request (522 B): `[op=1 (2B)] [sector (8B)] [data (512B)]`
//! Flush request   (2 B): `[op=2 (2B)]`
//!
//! Read reply OK  (513 B): `[0x00] [sector_data (512B)]`
//! Write reply OK   (1 B): `[0x00]`
//! Flush reply OK   (1 B): `[0x00]`
//! Error reply      (1 B): `[0x01]`
//!
//! Every sector is an absolute LBA on the whole device, which for a single
//! AHCI port is exactly the disk LBA space.

use crate::controller::AhciController;
use crate::dma::AuthorizedDma;
use ostd::dma::DmaBuf;

/// Total reply buffer size: status byte + one full 512-byte sector.
pub const REPLY_SIZE: usize = 513;

#[derive(Debug, PartialEq, Eq)]
pub enum DrvOp {
    Read = 0,
    Write = 1,
    Flush = 2,
}

fn parse_op(data: &[u8]) -> Option<(DrvOp, u64)> {
    if data.len() < 2 {
        return None;
    }
    let op = match u16::from_le_bytes([data[0], data[1]]) {
        0 => DrvOp::Read,
        1 => DrvOp::Write,
        2 => return Some((DrvOp::Flush, 0)),
        _ => return None,
    };
    if data.len() < 10 {
        return None;
    }
    let sector = u64::from_le_bytes(data[2..10].try_into().ok()?);
    Some((op, sector))
}

/// Handle one incoming IPC message. Writes the reply into `out` and returns
/// the number of bytes to send back.
///
/// Read OK:  writes `[0x00] ++ sector_data`, returns 513.
/// Write OK: writes `[0x00]`, returns 1.
/// Error:    writes `[0x01]`, returns 1.
pub fn handle(
    ctrl: &mut AhciController,
    io_buf: &AuthorizedDma<DmaBuf>,
    data: &[u8],
    out: &mut [u8; REPLY_SIZE],
) -> usize {
    let (op, sector) = match parse_op(data) {
        Some(v) => v,
        None => {
            out[0] = 1;
            return 1;
        }
    };

    match op {
        DrvOp::Read => match ctrl.read_sector(sector, io_buf.iova()) {
            Ok(_) => {
                out[0] = 0;
                // SAFETY: read_sector completed; the DMA payload is now stable in
                // the CPU mapping retained by io_buf, which covers one page.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        io_buf.inner().virt(),
                        out[1..].as_mut_ptr(),
                        512,
                    );
                }
                513
            }
            Err(_) => {
                out[0] = 1;
                1
            }
        },

        DrvOp::Write => {
            if data.len() < 10 + 512 {
                out[0] = 1;
                return 1;
            }
            // SAFETY: copy the caller payload into the CPU mapping before
            // submitting the authorized device-visible IOVA.
            unsafe {
                core::ptr::copy_nonoverlapping(data[10..].as_ptr(), io_buf.inner().virt(), 512);
            }
            match ctrl.write_sector(sector, io_buf.iova()) {
                Ok(_) => {
                    out[0] = 0;
                    1
                }
                Err(_) => {
                    out[0] = 1;
                    1
                }
            }
        }

        DrvOp::Flush => match ctrl.flush() {
            Ok(_) => {
                out[0] = 0;
                1
            }
            Err(_) => {
                out[0] = 1;
                1
            }
        },
    }
}
