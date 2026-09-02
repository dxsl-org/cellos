//! TLS 1.3 client helpers for app cells.
//!
//! Wraps the raw TLS IPC opcodes (0x30–0x32) exposed by the net service cell
//! into ergonomic functions.  All helpers are blocking; they send one IPC
//! message and wait for the reply.
//!
//! ## Usage
//! ```no_run
//! use ostd::service::{lookup, service};
//! use ostd::tls::{tls_close, tls_connect, tls_read, tls_write};
//!
//! let net = lookup(service::NET).expect("net service not ready");
//! let cap = tls_connect(net, [93, 184, 216, 34], 443, "example.com");
//! if cap == 0 { /* handle error */ }
//!
//! tls_write(net, cap, b"GET / HTTP/1.0\r\n\r\n");
//! let mut resp = [0u8; 512];
//! let n = tls_read(net, cap, &mut resp);
//! tls_close(net, cap);
//! ```

extern crate alloc;

use crate::syscall::{sys_recv, sys_send, SyscallResult};

// TLS IPC opcodes (mirrors cells/services/net/src/poll_driver.rs cell_opcodes).
const TLS_CONNECT: u8 = 0x30;
const TLS_SEND: u8 = 0x31;
const TLS_RECV: u8 = 0x32;
const CLOSE: u8 = 0x15;

/// Open a TLS 1.3 connection to `addr:port` with the given SNI `hostname`.
///
/// Sends `TLS_CONNECT` to the net service at `net_tid`.  Blocks until the TCP
/// handshake and TLS handshake complete, or until the net service times out.
///
/// Returns the `cap_id` (non-zero on success, 0 on failure).
pub fn tls_connect(net_tid: usize, addr: [u8; 4], port: u16, hostname: &str) -> u64 {
    // Wire format: [0x30][cap:8=0][addr:4][port:2 LE][hn_len:2 LE][hostname bytes]
    let hn = hostname.as_bytes();
    let hn_len = hn.len().min(495);
    let msg_len = 17 + hn_len;
    let mut msg = alloc::vec![0u8; msg_len];
    msg[0] = TLS_CONNECT;
    // cap placeholder (bytes 1-8): 0
    msg[9..13].copy_from_slice(&addr);
    msg[13..15].copy_from_slice(&port.to_le_bytes());
    msg[15..17].copy_from_slice(&(hn_len as u16).to_le_bytes());
    msg[17..17 + hn_len].copy_from_slice(&hn[..hn_len]);

    sys_send(net_tid, &msg);
    let mut reply = [0u8; 8];
    match sys_recv(0, &mut reply) {
        SyscallResult::Ok(_) => u64::from_le_bytes(reply),
        _ => 0,
    }
}

/// Write data to an established TLS connection.
///
/// Returns the number of bytes accepted by the net service (may be less than
/// `data.len()` if the send buffer is temporarily full — retry on partial write).
pub fn tls_write(net_tid: usize, cap_id: u64, data: &[u8]) -> usize {
    // Wire format: [0x31][cap:8 LE][len:2 LE][data:*]
    let payload_len = data.len().min(501); // 512 - 11 header bytes
    let msg_len = 11 + payload_len;
    let mut msg = alloc::vec![0u8; msg_len];
    msg[0] = TLS_SEND;
    msg[1..9].copy_from_slice(&cap_id.to_le_bytes());
    msg[9..11].copy_from_slice(&(payload_len as u16).to_le_bytes());
    msg[11..11 + payload_len].copy_from_slice(&data[..payload_len]);
    sys_send(net_tid, &msg);
    let mut reply = [0u8; 4];
    match sys_recv(0, &mut reply) {
        SyscallResult::Ok(_) => u32::from_le_bytes(reply) as usize,
        _ => 0,
    }
}

/// Read decrypted data from an established TLS connection.
///
/// Returns the number of bytes written into `buf`.  Returns 0 if no data is
/// available yet (non-blocking on the server side; the TLS transport itself
/// blocks until the next TLS record arrives, so the call may take a while).
pub(crate) fn parse_tls_read_reply(reply: &[u8], buf: &mut [u8]) -> usize {
    if reply.len() < 2 {
        return 0;
    }
    let declared_len = u16::from_le_bytes([reply[0], reply[1]]) as usize;
    let available = reply.len() - 2;
    if declared_len > buf.len() || declared_len > available {
        return 0;
    }
    buf[..declared_len].copy_from_slice(&reply[2..2 + declared_len]);
    declared_len
}

pub fn tls_read(net_tid: usize, cap_id: u64, buf: &mut [u8]) -> usize {
    // Wire format: [0x32][cap:8 LE][buf_len:4 LE]
    let mut msg = [0u8; 13];
    msg[0] = TLS_RECV;
    let cap_bytes = cap_id.to_le_bytes();
    msg[1..9].copy_from_slice(&cap_bytes);
    let want_len = buf.len().min(4094);
    let want = (want_len as u32).to_le_bytes();
    msg[9..13].copy_from_slice(&want);

    sys_send(net_tid, &msg);
    let mut tmp = alloc::vec![0u8; 2 + want_len];
    match sys_recv(0, &mut tmp) {
        SyscallResult::Ok(_) => parse_tls_read_reply(&tmp, buf),
        _ => 0,
    }
}

/// Close a TLS connection (also removes the underlying TCP socket).
pub fn tls_close(net_tid: usize, cap_id: u64) {
    let mut msg = [0u8; 9];
    msg[0] = CLOSE;
    msg[1..9].copy_from_slice(&cap_id.to_le_bytes());
    sys_send(net_tid, &msg);
    let mut r = [0u8; 1];
    let _ = sys_recv(0, &mut r);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reply_valid_and_trailing_zeros() {
        let binary = b"hello\x00\x00\x00";
        let mut reply = alloc::vec![0u8; 2 + binary.len()];
        reply[0..2].copy_from_slice(&(binary.len() as u16).to_le_bytes());
        reply[2..].copy_from_slice(binary);

        let mut out = [0u8; 32];
        let n = parse_tls_read_reply(&reply, &mut out);
        assert_eq!(n, 8);
        assert_eq!(&out[..8], binary);
    }

    #[test]
    fn parse_reply_negative_cases() {
        let mut out = [0u8; 32];

        // Truncated header (< 2 bytes)
        assert_eq!(parse_tls_read_reply(&[], &mut out), 0);
        assert_eq!(parse_tls_read_reply(&[0x05], &mut out), 0);

        // Empty payload (0 bytes)
        assert_eq!(parse_tls_read_reply(&[0x00, 0x00], &mut out), 0);

        // Declared length exceeds available buffer -> fail closed (0)
        let mut bad_reply = [0u8; 6];
        bad_reply[0..2].copy_from_slice(&100u16.to_le_bytes()); // says 100, only 4 follow
        bad_reply[2..].copy_from_slice(b"test");
        assert_eq!(parse_tls_read_reply(&bad_reply, &mut out), 0);

        // Declared length exceeds destination buffer -> fail closed (0)
        let mut small_out = [0u8; 2];
        assert_eq!(parse_tls_read_reply(&bad_reply, &mut small_out), 0);
    }
}
