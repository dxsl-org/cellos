//! IPC request dispatcher — translates `DrvRequest` messages into NVMe I/O.
//!
//! Wire format (little-endian):
//!
//! Read request  (10 B): `[op=0 (2B)] [sector (8B)]`
//! Write request (522 B): `[op=1 (2B)] [sector (8B)] [data (512B)]`
//!
//! Read reply OK  (513 B): `[0x00] [sector_data (512B)]`
//! Write reply OK   (1 B): `[0x00]`
//! Error reply      (1 B): `[0x01]`

use ostd::dma::DmaBuf;
use crate::controller::NvmeController;

/// Total reply buffer size: status byte + one full 512-byte sector.
pub const REPLY_SIZE: usize = 513;

#[derive(Debug, PartialEq, Eq)]
pub enum DrvOp { Read = 0, Write = 1 }

fn parse_op(data: &[u8]) -> Option<(DrvOp, u64)> {
    if data.len() < 10 { return None; }
    let op = match u16::from_le_bytes([data[0], data[1]]) {
        0 => DrvOp::Read,
        1 => DrvOp::Write,
        _ => return None,
    };
    let sector = u64::from_le_bytes(data[2..10].try_into().ok()?);
    Some((op, sector))
}

/// Encode a Read IPC request (10 bytes). Used by VFS.
pub fn encode_read(sector: u64) -> [u8; 10] {
    let mut b = [0u8; 10];
    b[0..2].copy_from_slice(&0u16.to_le_bytes());
    b[2..10].copy_from_slice(&sector.to_le_bytes());
    b
}

/// Encode a Write IPC request (522 bytes). Used by VFS.
pub fn encode_write(sector: u64, data: &[u8; 512]) -> [u8; 522] {
    let mut b = [0u8; 522];
    b[0..2].copy_from_slice(&1u16.to_le_bytes());
    b[2..10].copy_from_slice(&sector.to_le_bytes());
    b[10..522].copy_from_slice(data);
    b
}

/// Handle one incoming IPC message. Writes the reply into `out` and returns
/// the number of bytes to send back.
///
/// Read OK:  writes `[0x00] ++ sector_data`, returns 513.
/// Write OK: writes `[0x00]`, returns 1.
/// Error:    writes `[0x01]`, returns 1.
pub fn handle(
    ctrl:   &mut NvmeController,
    io_buf: &DmaBuf,
    data:   &[u8],
    out:    &mut [u8; REPLY_SIZE],
) -> usize {
    let (op, sector) = match parse_op(data) {
        Some(v) => v,
        None    => { out[0] = 1; return 1; }
    };

    match op {
        DrvOp::Read => {
            match ctrl.read_sector(sector, io_buf.phys() as u64) {
                Ok(_) => {
                    out[0] = 0;
                    // SAFETY: io_buf is identity-mapped (phys == virt) in SAS.
                    // read_sector completed; DMA data is now stable in io_buf.
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            io_buf.virt(),
                            out[1..].as_mut_ptr(),
                            512,
                        );
                    }
                    513
                }
                Err(_) => { out[0] = 1; 1 }
            }
        }

        DrvOp::Write => {
            if data.len() < 10 + 512 { out[0] = 1; return 1; }
            // SAFETY: io_buf is identity-mapped; we write the caller's payload
            // in before handing the physical address to the NVMe controller.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    data[10..].as_ptr(),
                    io_buf.virt(),
                    512,
                );
            }
            match ctrl.write_sector(sector, io_buf.phys() as u64) {
                Ok(_)  => { out[0] = 0; 1 }
                Err(_) => { out[0] = 1; 1 }
            }
        }
    }
}
