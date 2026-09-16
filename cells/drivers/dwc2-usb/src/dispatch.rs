//! IPC request dispatcher for the LAN9514 NIC Driver Cell.
//!
//! Implements the raw NIC wire protocol (shared with virtio-net and e1000):
//!   OP_TX (0):   [0x00, len_lo, len_hi] ++ frame_bytes -> [0x00] OK / [0x01] Err
//!   OP_RX (1):   [0x01] -> [len_lo, len_hi] ++ frame_bytes
//!   OP_GETMAC (2): [0x02] -> 6 MAC bytes

use crate::lan9514::Lan9514Device;

const OP_TX: u8 = 0;
const OP_RX: u8 = 1;
const OP_GETMAC: u8 = 2;

pub const FRAME_BUF: usize = 1514;
pub const REPLY_BUF: usize = 2 + FRAME_BUF;

pub enum NicReply<'a> {
    Status(u8),
    Frame { len: usize, buf: &'a mut [u8] },
    Mac([u8; 6]),
}

pub fn handle<'a>(
    dev: &mut Lan9514Device<'_>,
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
            if len == 0 || len > FRAME_BUF || (3 + len) > data.len() {
                return NicReply::Status(1);
            }
            let frame = &data[3..3 + len];
            let ok = dev.send_frame(frame);
            if ok {
                ostd::io::println("[dwc2-usb] TX packet transmitted OK");
                NicReply::Status(0)
            } else {
                NicReply::Status(1)
            }
        }

        OP_RX => {
            let n = dev.recv_frame(&mut out_buf[2..]);
            out_buf[0] = (n & 0xFF) as u8;
            out_buf[1] = ((n >> 8) & 0xFF) as u8;
            NicReply::Frame {
                len: n,
                buf: out_buf,
            }
        }

        OP_GETMAC => NicReply::Mac(dev.mac_address()),

        _ => NicReply::Status(1),
    }
}
