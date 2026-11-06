//! IPC request dispatcher for the vmxnet3 NIC Driver Cell — the unchanged NIC
//! wire `/bin/e1000` and `/bin/igb` serve (`cells/services/net/src/interface.rs`).
//!
//! Wire format (little-endian):
//!   byte 0: op  (0 = Tx, 1 = Rx, 2 = GetMac)
//!   byte 1+: payload
//!
//! Tx request:   [0x00, len_lo, len_hi] ++ frame_bytes (Ethernet frame, no FCS)
//!   The explicit length is REQUIRED: raw IPC delivery hands the receiver its
//!   whole 4096-byte recv buffer with no byte count, so the frame boundary
//!   cannot be inferred from the message itself.
//! Rx request:   [0x01]
//! GetMac:       [0x02]
//!
//! Reply for Tx:     [0x00] = OK, [0x01] = Error
//! Reply for Rx:     [len_lo, len_hi] ++ frame_bytes  (len=0 → nothing ready)
//! Reply for GetMac: 6 MAC bytes

use crate::controller::Vmxnet3;
use crate::rings::BUF_SIZE;
use ostd::service::{sender_is_live_provider, service};
use ostd::sync::Mutex;
use ostd::syscall::sys_try_send;

const OP_TX: u8 = 0;
const OP_RX: u8 = 1;
const OP_GETMAC: u8 = 2;

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
    ctrl: &mut Vmxnet3,
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
        _ => NicReply::Status(1),
    }
}

/// Serve one request from `sender_tid` and reply on the operation (or with a
/// `try_send` for a sender that did not use one).
///
/// Only the live net service may drive the NIC; `sender_tid` is kernel-stamped
/// and the registry is asked per request, so a restarted net service is
/// followed without a cached tid. Refusal is the ordinary failure byte the net
/// service retries on.
pub fn serve(state: &Mutex<Option<Vmxnet3>>, sender_tid: usize, data: &[u8]) {
    let operation = ostd::ipc::current();
    let reply = |bytes: &[u8]| {
        if let Some(operation) = operation {
            let _ = ostd::ipc::reply(operation, bytes);
        } else {
            let _ = sys_try_send(sender_tid, bytes);
        }
    };
    if !sender_is_live_provider(service::NET, sender_tid) {
        reply(&[1u8]);
        return;
    }
    let mut out_buf = [0u8; REPLY_BUF];
    match state.lock().as_mut() {
        Some(ctrl) => match handle(ctrl, data, &mut out_buf) {
            NicReply::Status(code) => reply(&[code]),
            NicReply::Frame { len, buf } => reply(&buf[..2 + len]),
            NicReply::Raw(mac) => reply(&mac),
        },
        None => reply(&[1u8]),
    }
}

/// `aa:bb:cc:dd:ee:ff` for the bind log line.
pub struct MacFmt(pub [u8; 6]);

impl core::fmt::Display for MacFmt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{g:02x}")
    }
}
