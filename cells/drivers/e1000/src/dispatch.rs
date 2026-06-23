//! IPC request dispatcher for the e1000 NIC Driver Cell.
//!
//! Wire format (little-endian):
//!   byte 0: op  (0 = Tx, 1 = Rx, 2 = GetMac)
//!   byte 1+: payload
//!
//! Tx request:   [0x00] ++ frame_bytes (Ethernet frame, no FCS)
//! Rx request:   [0x01]
//! GetMac:       [0x02]
//!
//! Reply for Tx:     [0x00] = OK, [0x01] = Error
//! Reply for Rx:     [len_lo, len_hi] ++ frame_bytes  (len=0 → nothing ready)
//! Reply for GetMac: 6 MAC bytes

use crate::controller::{E1000Controller, BUF_SIZE};

const OP_TX:     u8 = 0;
const OP_RX:     u8 = 1;
const OP_GETMAC: u8 = 2;

/// Total output buffer size: 2-byte length header + full frame payload.
pub const REPLY_BUF: usize = 2 + BUF_SIZE;

pub enum NicReply<'a> {
    /// Single byte status (Tx OK/Err, or unknown op).
    Status(u8),
    /// [len_lo, len_hi] ++ frame slice (Rx result).
    Frame { len: usize, buf: &'a mut [u8] },
    /// Raw bytes (GetMac).
    Raw([u8; 6]),
}

/// Handle one incoming IPC message.
///
/// `out_buf` must be exactly `REPLY_BUF` bytes (2-byte header + frame).
/// Returns a `NicReply` describing what to send back.
pub fn handle<'a>(
    ctrl:    &mut E1000Controller,
    data:    &[u8],
    out_buf: &'a mut [u8; REPLY_BUF],
) -> NicReply<'a> {
    if data.is_empty() { return NicReply::Status(1); }
    match data[0] {
        OP_TX => {
            let frame = &data[1..];
            if frame.is_empty() { return NicReply::Status(1); }
            match ctrl.send_frame(frame) {
                Ok(_)  => NicReply::Status(0),
                Err(_) => NicReply::Status(1),
            }
        }
        OP_RX => {
            // First 2 bytes of out_buf = frame length (0 if nothing).
            let n = ctrl.recv_frame(&mut out_buf[2..]);
            out_buf[0] = (n & 0xFF) as u8;
            out_buf[1] = ((n >> 8) & 0xFF) as u8;
            NicReply::Frame { len: n, buf: out_buf }
        }
        OP_GETMAC => {
            NicReply::Raw(ctrl.mac)
        }
        _ => NicReply::Status(1),
    }
}
