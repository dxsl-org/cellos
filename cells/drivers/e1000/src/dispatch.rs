//! IPC request dispatcher for the e1000 NIC Driver Cell.
//!
//! Serves the NIC wire defined in [`wire::proto::nic`].

use crate::controller::{E1000Controller, BUF_SIZE};
use wire::proto::nic::{NicStatus, OP_GETMAC, OP_RX, OP_TX};

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
    ctrl: &mut E1000Controller,
    data: &[u8],
    out_buf: &'a mut [u8; REPLY_BUF],
) -> NicReply<'a> {
    if data.is_empty() {
        return NicReply::Status(NicStatus::Failed as u8);
    }
    match data[0] {
        OP_TX => {
            // [op, len_lo, len_hi, frame...] — length header bounds the frame
            // inside the (padded) IPC buffer.
            if data.len() < 3 {
                return NicReply::Status(NicStatus::Failed as u8);
            }
            let len = u16::from_le_bytes([data[1], data[2]]) as usize;
            if len == 0 || len > BUF_SIZE || 3 + len > data.len() {
                return NicReply::Status(NicStatus::Failed as u8);
            }
            let frame = &data[3..3 + len];
            match ctrl.send_frame(frame) {
                Ok(_) => NicReply::Status(NicStatus::Ok as u8),
                Err(_) => NicReply::Status(NicStatus::Failed as u8),
            }
        }
        OP_RX => {
            // First 2 bytes of out_buf = frame length (0 if nothing).
            let n = ctrl.recv_frame(&mut out_buf[2..]);
            out_buf[0] = (n & 0xFF) as u8;
            out_buf[1] = ((n >> 8) & 0xFF) as u8;
            NicReply::Frame {
                len: n,
                buf: out_buf,
            }
        }
        OP_GETMAC => NicReply::Raw(ctrl.mac),
        _ => NicReply::Status(NicStatus::Failed as u8),
    }
}
