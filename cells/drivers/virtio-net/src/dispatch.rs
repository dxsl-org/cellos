//! IPC request dispatcher for the virtio-net Driver Cell.
//!
//! Wire format (little-endian) — identical to e1000 so `net` service IPC is shared:
//!   byte 0: op  (0 = Tx, 1 = Rx, 2 = GetMac)
//!   byte 1+: payload
//!
//! Tx request:   [0x00] ++ frame_bytes
//! Rx request:   [0x01]
//! GetMac:       [0x02]
//!
//! Reply Tx:     [0x00] OK  /  [0x01] Error
//! Reply Rx:     [len_lo, len_hi] ++ frame_bytes   (len=0 → nothing ready)
//! Reply GetMac: 6 MAC bytes

use crate::device::NetDevice;

const OP_TX:     u8 = 0;
const OP_RX:     u8 = 1;
const OP_GETMAC: u8 = 2;

/// Max frame size (1514 bytes Ethernet + 2-byte length header).
pub const FRAME_BUF: usize = 1514;
/// Total reply buffer: 2-byte length prefix + max frame.
pub const REPLY_BUF: usize = 2 + FRAME_BUF;

pub enum NicReply<'a> {
    /// Single-byte status (Tx OK/Err, unknown op).
    Status(u8),
    /// [len_lo, len_hi] ++ frame slice (Rx result; len=0 = empty).
    Frame { len: usize, buf: &'a mut [u8] },
    /// Raw MAC bytes (GetMac).
    Mac([u8; 6]),
}

/// Handle one incoming IPC message from the net service.
///
/// `out_buf` must be `REPLY_BUF` bytes; returns a `NicReply` describing the response.
pub fn handle<'a>(
    dev:     &mut NetDevice,
    data:    &[u8],
    out_buf: &'a mut [u8; REPLY_BUF],
) -> NicReply<'a> {
    if data.is_empty() { return NicReply::Status(1); }

    match data[0] {
        OP_TX => {
            let frame = &data[1..];
            if frame.is_empty() { return NicReply::Status(1); }
            if dev.send(frame) { NicReply::Status(0) } else { NicReply::Status(1) }
        }

        OP_RX => {
            // 1. Try immediate receive.
            let mut n = dev.try_recv(&mut out_buf[2..]);

            // 2. If nothing ready, block on IRQ then try once more.
            if n == 0 {
                n = dev.wait_recv(&mut out_buf[2..]);
            }

            out_buf[0] = (n & 0xFF) as u8;
            out_buf[1] = ((n >> 8) & 0xFF) as u8;
            NicReply::Frame { len: n, buf: out_buf }
        }

        OP_GETMAC => NicReply::Mac(dev.mac()),

        _ => NicReply::Status(1),
    }
}
