//! IPC request dispatcher for the virtio-net Driver Cell.
//!
//! Wire format (little-endian) — identical to e1000 so `net` service IPC is shared:
//!   byte 0: op  (0 = Tx, 1 = Rx, 2 = GetMac)
//!   byte 1+: payload
//!
//! Tx request:   [0x00, len_lo, len_hi] ++ frame_bytes
//!   The explicit length is REQUIRED: raw IPC delivery hands the receiver its
//!   whole 4096-byte recv buffer with no byte count, so the frame boundary
//!   cannot be inferred from the message itself.
//! Rx request:   [0x01]
//! GetMac:       [0x02]
//!
//! Reply Tx:     [0x00] OK  /  [0x01] Error
//! Reply Rx:     [len_lo, len_hi] ++ frame_bytes   (len=0 → nothing ready)
//! Reply GetMac: 6 MAC bytes

use crate::device::NetDevice;

const OP_TX: u8 = 0;
const OP_RX: u8 = 1;
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
    dev: &mut NetDevice,
    data: &[u8],
    out_buf: &'a mut [u8; REPLY_BUF],
) -> NicReply<'a> {
    if data.is_empty() {
        return NicReply::Status(1);
    }

    match data[0] {
        OP_TX => {
            // [op, len_lo, len_hi, frame...] — length header bounds the frame
            // inside the (padded) IPC buffer.
            if data.len() < 3 {
                return NicReply::Status(1);
            }
            let len = u16::from_le_bytes([data[1], data[2]]) as usize;
            if len == 0 || len > FRAME_BUF || 3 + len > data.len() {
                return NicReply::Status(1);
            }
            let frame = &data[3..3 + len];
            if dev.send(frame) {
                NicReply::Status(0)
            } else {
                NicReply::Status(1)
            }
        }

        OP_RX => {
            // Non-blocking: return whatever is in the VirtIO RX ring right now (0 = empty).
            // The net service's sys_wait_for_event(NET_RX) path handles blocking-wait
            // for the next packet; blocking here would deadlock pump_rx_split when the
            // second iteration finds an empty ring while a pending TcpSend is queued.
            let n = dev.try_recv(&mut out_buf[2..]);
            out_buf[0] = (n & 0xFF) as u8;
            out_buf[1] = ((n >> 8) & 0xFF) as u8;
            NicReply::Frame {
                len: n,
                buf: out_buf,
            }
        }

        OP_GETMAC => NicReply::Mac(dev.mac()),

        _ => NicReply::Status(1),
    }
}
