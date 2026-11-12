//! IPC request dispatcher for the LAN9514 NIC Driver Cell.
//!
//! Serves the NIC wire defined in [`api::services::proto::nic`], shared with
//! virtio-net and e1000. Only [`NicStatus::Ok`] means the frame went out; every
//! other status is a refusal the Net Cell retries, and the value names which
//! kind (the chip refused versus the driver was not ready).

use crate::lan9514::Lan9514Device;
use api::services::proto::nic::{NicStatus, OP_GETMAC, OP_RX, OP_TX};

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
        return NicReply::Status(NicStatus::Failed as u8);
    }

    match data[0] {
        OP_TX => {
            if data.len() < 3 {
                return NicReply::Status(NicStatus::Failed as u8);
            }
            let len = u16::from_le_bytes([data[1], data[2]]) as usize;
            if len == 0 || len > FRAME_BUF || (3 + len) > data.len() {
                return NicReply::Status(NicStatus::Failed as u8);
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
                NicReply::Status(NicStatus::Ok as u8)
            } else {
                NicReply::Status(NicStatus::Failed as u8)
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

        _ => NicReply::Status(NicStatus::Failed as u8),
    }
}
