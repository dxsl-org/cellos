//! IPC request dispatcher for the LAN9514 NIC Driver Cell.
//!
//! Implements the raw NIC wire protocol (shared with virtio-net and e1000):
//!   OP_TX (0):   [0x00, len_lo, len_hi] ++ frame_bytes -> [0x00] OK / [0x01] Err
//!   OP_RX (1):   [0x01] -> [len_lo, len_hi] ++ frame_bytes
//!   OP_GETMAC (2): [0x02] -> 6 MAC bytes

use crate::lan9514::Lan9514Device;

pub const OP_TX: u8 = 0;
pub const OP_RX: u8 = 1;
pub const OP_GETMAC: u8 = 2;

/// NIC response status byte: only [`STATUS_OK`] means the frame went out.
///
/// Every other value is a refusal the caller may retry, and the value says which
/// kind — the board's ping drowned in a single `accepted=false` byte whose cause
/// (the chip refused versus the driver was not ready) had to be guessed from the
/// surrounding lines. The Net Cell keeps a refused frame and offers it again,
/// bounded, so naming the refusal is what decides the next fix.
pub const STATUS_OK: u8 = 0;
/// The request was malformed, or its USB transfer failed.
pub const STATUS_FAILED: u8 = 1;
/// The front-end that decodes requests was not parked: nothing reached the chip.
pub const STATUS_NOT_READY: u8 = 2;

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
        return NicReply::Status(STATUS_FAILED);
    }

    match data[0] {
        OP_TX => {
            if data.len() < 3 {
                return NicReply::Status(STATUS_FAILED);
            }
            let len = u16::from_le_bytes([data[1], data[2]]) as usize;
            if len == 0 || len > FRAME_BUF || (3 + len) > data.len() {
                return NicReply::Status(STATUS_FAILED);
            }
            let frame = &data[3..3 + len];
            let ok = dev.send_frame(frame);
            if ok {
                // One line per *successful* frame would flood the console the
                // guest is being driven from (the LAN carries one per ARP/echo
                // exchange). The first success is the fact worth a witness; the
                // count is what `[net-loop] drv_cmd` is for.
                static FIRST_TX_OK: core::sync::atomic::AtomicBool =
                    core::sync::atomic::AtomicBool::new(false);
                if !FIRST_TX_OK.swap(true, core::sync::atomic::Ordering::Relaxed) {
                    ostd::io::println("[dwc2-usb] TX packet transmitted OK");
                }
                NicReply::Status(STATUS_OK)
            } else {
                NicReply::Status(STATUS_FAILED)
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

        _ => NicReply::Status(STATUS_FAILED),
    }
}
