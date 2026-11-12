//! IPC request dispatcher for the Intel i40e NIC Driver Cell.
//!
//! Serves the NIC wire defined in [`api::services::proto::nic`].

use crate::controller::{I40eController, BUF_SIZE};
use api::services::proto::nic::{NicStatus, OP_GETMAC, OP_RX, OP_TX};

pub const REPLY_BUF: usize = 2 + BUF_SIZE;

pub enum NicReply<'a> {
    Status(u8),
    Frame { len: usize, buf: &'a mut [u8] },
    Raw([u8; 6]),
}

pub fn handle<'a>(
    ctrl: &mut I40eController,
    data: &[u8],
    out_buf: &'a mut [u8; REPLY_BUF],
) -> NicReply<'a> {
    if data.is_empty() {
        return NicReply::Status(NicStatus::Failed as u8);
    }
    match data[0] {
        OP_TX => {
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
            let n = ctrl.poll_rx(&mut out_buf[2..]).unwrap_or_default();
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
