//! IPC request dispatcher — translates `DrvRequest` messages into VirtIO-blk I/O.
//!
//! Wire format (little-endian) — IDENTICAL to the NVMe Driver Cell so VFS speaks
//! one block protocol (`cells/services/vfs/src/block_stream.rs`):
//!
//! Read request  (10 B):  `[op=0 (2B)] [sector (8B)]`
//! Write request (522 B): `[op=1 (2B)] [sector (8B)] [data (512B)]`
//!
//! Read reply OK  (513 B): `[0x00] [sector_data (512B)]`
//! Write reply OK   (1 B): `[0x00]`
//! Error reply      (1 B): `[0x01]`

use crate::device::BlkDevice;

/// Reply buffer size: status byte + one full 512-byte sector.
pub const REPLY_SIZE: usize = 513;

const SECTOR_SIZE: usize = 512;

#[derive(PartialEq, Eq)]
enum DrvOp {
    Read = 0,
    Write = 1,
}

fn parse_op(data: &[u8]) -> Option<(DrvOp, u64)> {
    if data.len() < 10 {
        return None;
    }
    let op = match u16::from_le_bytes([data[0], data[1]]) {
        0 => DrvOp::Read,
        1 => DrvOp::Write,
        _ => return None,
    };
    let sector = u64::from_le_bytes(data[2..10].try_into().ok()?);
    Some((op, sector))
}

/// Handle one incoming IPC message. Writes the reply into `out` and returns the
/// number of bytes to send back.
///
/// Read OK:  `[0x00] ++ sector_data`, returns 513.
/// Write OK: `[0x00]`, returns 1.
/// Error:    `[0x01]`, returns 1.
pub fn handle(dev: &mut BlkDevice, data: &[u8], out: &mut [u8; REPLY_SIZE]) -> usize {
    let (op, sector) = match parse_op(data) {
        Some(v) => v,
        None => {
            out[0] = 1;
            return 1;
        }
    };

    match op {
        DrvOp::Read => {
            let mut sec = [0u8; SECTOR_SIZE];
            if dev.read_sector(sector, &mut sec) {
                out[0] = 0;
                out[1..1 + SECTOR_SIZE].copy_from_slice(&sec);
                REPLY_SIZE
            } else {
                out[0] = 1;
                1
            }
        }
        DrvOp::Write => {
            if data.len() < 10 + SECTOR_SIZE {
                out[0] = 1;
                return 1;
            }
            let mut sec = [0u8; SECTOR_SIZE];
            sec.copy_from_slice(&data[10..10 + SECTOR_SIZE]);
            if dev.write_sector(sector, &sec) {
                out[0] = 0;
                1
            } else {
                out[0] = 1;
                1
            }
        }
    }
}
