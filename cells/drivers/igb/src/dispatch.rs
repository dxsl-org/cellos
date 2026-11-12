//! IPC request dispatcher for the igb NIC Driver Cell.
//!
//! It serves the NIC wire in [`api::services::proto::nic`], so the net service
//! consumes this cell exactly as it consumes `/bin/e1000` — one network
//! surface, no device-specific path.

use crate::controller::{IgbController, BUF_SIZE};
use api::services::proto::nic::{NicStatus, OP_GETMAC, OP_RX, OP_TX};

/// Total output buffer size: 2-byte length header + full frame payload.
pub const REPLY_BUF: usize = 2 + BUF_SIZE;

pub enum NicReply<'a> {
    /// Single byte status (Tx OK/Err, or unknown op).
    Status(u8),
    /// `[len_lo, len_hi] ++ frame` slice (Rx result).
    Frame { len: usize, buf: &'a mut [u8] },
    /// Raw bytes (GetMac).
    Raw([u8; 6]),
}

/// Handle one incoming IPC message.
///
/// `out_buf` must be exactly `REPLY_BUF` bytes (2-byte header + frame).
pub fn handle<'a>(
    ctrl: &mut IgbController,
    data: &[u8],
    out_buf: &'a mut [u8; REPLY_BUF],
) -> NicReply<'a> {
    if data.is_empty() {
        return NicReply::Status(NicStatus::Failed as u8);
    }
    match data[0] {
        OP_TX => {
            // The explicit length header bounds the frame inside the (padded)
            // IPC buffer: raw delivery hands the receiver its whole recv buffer
            // with no byte count.
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
            // First 2 bytes of out_buf = frame length (0 if nothing ready).
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
