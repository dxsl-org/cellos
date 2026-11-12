//! IPC request dispatcher for the virtio-net Driver Cell.
//!
//! Serves the NIC wire in [`api::services::proto::nic`], shared with e1000 so
//! the `net` service IPC is the same for every NIC.

use crate::device::NetDevice;
use api::services::proto::nic::{NicStatus, OP_GETMAC, OP_RX, OP_TX};

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
            if len == 0 || len > FRAME_BUF || 3 + len > data.len() {
                return NicReply::Status(NicStatus::Failed as u8);
            }
            let frame = &data[3..3 + len];
            if dev.send(frame) {
                NicReply::Status(NicStatus::Ok as u8)
            } else {
                NicReply::Status(NicStatus::Failed as u8)
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

        _ => NicReply::Status(NicStatus::Failed as u8),
    }
}
