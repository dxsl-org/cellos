//! IPC request dispatcher for the Broadcom BCM57xx NIC Driver Cell.

use crate::controller::{BcmController, BUF_SIZE};

const OP_TX: u8 = 0;
const OP_RX: u8 = 1;
const OP_GETMAC: u8 = 2;

pub const REPLY_BUF: usize = 2 + BUF_SIZE;

pub enum NicReply<'a> {
    Status(u8),
    Frame { len: usize, buf: &'a mut [u8] },
    Raw([u8; 6]),
}

pub fn handle<'a>(
    ctrl: &mut BcmController,
    data: &[u8],
    out_buf: &'a mut [u8; REPLY_BUF],
) -> NicReply<'a> {
    if data.is_empty() {
        return NicReply::Status(1);
    }
    match data[0] {
        OP_TX => {
            if data.len() < 3 {
                return NicReply::Status(1);
            }
            let len = u16::from_le_bytes([data[1], data[2]]) as usize;
            if len == 0 || len > BUF_SIZE || 3 + len > data.len() {
                return NicReply::Status(1);
            }
            let frame = &data[3..3 + len];
            match ctrl.send_frame(frame) {
                Ok(_) => NicReply::Status(0),
                Err(_) => NicReply::Status(1),
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
        _ => NicReply::Status(1),
    }
}
