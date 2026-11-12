//! TLS 1.3 client helpers for app cells.
//!
//! Wraps the raw TLS IPC frames ([`api::services::proto::tls`]) exposed by the
//! net service cell into ergonomic functions. Each helper is one exact operation bounded by a
//! tick budget just above the net service's own deadline for that request, so
//! the service's failure answer arrives before the budget ends; a dead or
//! stalled service yields the helper's failure value.
//!
//! ## Usage
//! ```no_run
//! use ostd::service::{provider_tid, service};
//! use ostd::tls::{tls_close, tls_connect, tls_read, tls_write};
//!
//! let net = provider_tid(service::NET).expect("net service not ready");
//! let cap = tls_connect(net, [93, 184, 216, 34], 443, "example.com");
//! if cap == 0 { /* handle error */ }
//!
//! tls_write(net, cap, b"GET / HTTP/1.0\r\n\r\n");
//! let mut resp = [0u8; 512];
//! let n = tls_read(net, cap, &mut resp);
//! tls_close(net, cap);
//! ```

extern crate alloc;

use crate::ipc::call_bytes;
use api::services::proto::tls as wire;

/// Budget of `tls_connect`: 50 s, above the net service's 15 s TCP connect plus
/// 30 s handshake deadlines.
const TLS_CONNECT_TICKS: u64 = 5_000;
/// Budget of `tls_write`, `tls_read` and `tls_close`: 35 s, above the net
/// service's 30 s deadline for one TLS operation.
const TLS_IO_TICKS: u64 = 3_500;

/// Open a TLS 1.3 connection to `addr:port` with the given SNI `hostname`.
///
/// Sends [`wire::CONNECT`] to the net service at `net_tid`.  Blocks until the TCP
/// handshake and TLS handshake complete, or until the net service times out.
///
/// Returns the `cap_id` (non-zero on success, 0 on failure).
pub fn tls_connect(net_tid: usize, addr: [u8; 4], port: u16, hostname: &str) -> u64 {
    // Wire format: [CONNECT][cap:8=0][addr:4][port:2 LE][hn_len:2 LE][hostname bytes]
    let hn = hostname.as_bytes();
    let hn_len = hn.len().min(wire::MAX_HOSTNAME);
    let msg_len = wire::CONNECT_HEADER_LEN + hn_len;
    let mut msg = alloc::vec![0u8; msg_len];
    msg[0] = wire::CONNECT;
    // cap placeholder (bytes 1-8): 0
    msg[9..13].copy_from_slice(&addr);
    msg[13..15].copy_from_slice(&port.to_le_bytes());
    msg[15..17].copy_from_slice(&(hn_len as u16).to_le_bytes());
    msg[wire::CONNECT_HEADER_LEN..wire::CONNECT_HEADER_LEN + hn_len].copy_from_slice(&hn[..hn_len]);

    let mut reply = [0u8; 8];
    match call_bytes(net_tid, &msg, &mut reply, TLS_CONNECT_TICKS) {
        Ok(&[a, b, c, d, e, f, g, h]) => u64::from_le_bytes([a, b, c, d, e, f, g, h]),
        _ => wire::CONNECT_FAILED,
    }
}

/// Write data to an established TLS connection.
///
/// Returns the number of bytes accepted by the net service (may be less than
/// `data.len()` if the send buffer is temporarily full — retry on partial write).
pub fn tls_write(net_tid: usize, cap_id: u64, data: &[u8]) -> usize {
    // Wire format: [SEND][cap:8 LE][len:2 LE][data:*]
    let payload_len = data.len().min(wire::MAX_SEND);
    let msg_len = wire::SEND_HEADER_LEN + payload_len;
    let mut msg = alloc::vec![0u8; msg_len];
    msg[0] = wire::SEND;
    msg[1..wire::HEADER_LEN].copy_from_slice(&cap_id.to_le_bytes());
    msg[wire::HEADER_LEN..wire::SEND_HEADER_LEN]
        .copy_from_slice(&(payload_len as u16).to_le_bytes());
    msg[wire::SEND_HEADER_LEN..wire::SEND_HEADER_LEN + payload_len]
        .copy_from_slice(&data[..payload_len]);
    let mut reply = [0u8; 4];
    match call_bytes(net_tid, &msg, &mut reply, TLS_IO_TICKS) {
        Ok(&[a, b, c, d]) => {
            let count = u32::from_le_bytes([a, b, c, d]) as usize;
            // The service reserves `wire::SEND_ERROR` for errors, never byte progress.
            if count <= payload_len {
                count
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// Read decrypted data from an established TLS connection.
///
/// Returns `Ok(0)` only for clean EOF. Service errors and malformed frames
/// remain errors; unavailable transport data stays pending in the NET service.
pub(crate) fn parse_tls_read_reply(reply: &[u8], buf: &mut [u8]) -> crate::ViResult<usize> {
    if reply.len() < 2 {
        return Err(crate::ViError::IO);
    }
    let declared_len = u16::from_le_bytes([reply[0], reply[1]]) as usize;
    if declared_len == wire::RECV_ERROR as usize {
        return Err(crate::ViError::IO);
    }
    let available = reply.len() - 2;
    if declared_len > buf.len() || declared_len > available {
        return Err(crate::ViError::IO);
    }
    buf[..declared_len].copy_from_slice(&reply[2..2 + declared_len]);
    Ok(declared_len)
}

pub fn tls_read(net_tid: usize, cap_id: u64, buf: &mut [u8]) -> crate::ViResult<usize> {
    if buf.is_empty() {
        return Ok(0);
    }
    // Wire format: [RECV][cap:8 LE][buf_len:4 LE]
    let mut msg = [0u8; wire::RECV_REQUEST_LEN];
    msg[0] = wire::RECV;
    let cap_bytes = cap_id.to_le_bytes();
    msg[1..wire::HEADER_LEN].copy_from_slice(&cap_bytes);
    let want_len = buf.len().min(wire::MAX_RECV_DATA);
    let want = (want_len as u32).to_le_bytes();
    msg[wire::HEADER_LEN..wire::RECV_REQUEST_LEN].copy_from_slice(&want);

    let mut tmp = alloc::vec![0u8; 2 + want_len];
    match call_bytes(net_tid, &msg, &mut tmp, TLS_IO_TICKS) {
        Ok(slice) => parse_tls_read_reply(slice, buf),
        Err(_) => Err(crate::ViError::IO),
    }
}

/// Close a TLS connection (also removes the underlying TCP socket).
pub fn tls_close(net_tid: usize, cap_id: u64) {
    let mut msg = [0u8; wire::HEADER_LEN];
    msg[0] = wire::CLOSE;
    msg[1..wire::HEADER_LEN].copy_from_slice(&cap_id.to_le_bytes());
    let mut r = [0u8; 1];
    let _ = call_bytes(net_tid, &msg, &mut r, TLS_IO_TICKS);
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
        let n = parse_tls_read_reply(&reply, &mut out).unwrap();
        assert_eq!(n, 8);
        assert_eq!(&out[..8], binary);
    }

    #[test]
    fn parse_reply_negative_cases() {
        let mut out = [0u8; 32];

        // Truncated header (< 2 bytes)
        assert!(parse_tls_read_reply(&[], &mut out).is_err());
        assert!(parse_tls_read_reply(&[0x05], &mut out).is_err());

        // Empty payload (0 bytes)
        assert_eq!(
            parse_tls_read_reply(&wire::RECV_EOF.to_le_bytes(), &mut out).unwrap(),
            0
        );
        assert!(parse_tls_read_reply(&wire::RECV_ERROR.to_le_bytes(), &mut out).is_err());

        // A short body is not EOF and must leave the output untouched.
        let mut bad_reply = [0u8; 6];
        bad_reply[0..2].copy_from_slice(&100u16.to_le_bytes()); // says 100, only 4 follow
        bad_reply[2..].copy_from_slice(b"test");
        assert!(parse_tls_read_reply(&bad_reply, &mut out).is_err());
        assert_eq!(out, [0; 32]);

        // A valid frame larger than the destination must not be partially copied.
        let mut small_out = [7u8; 2];
        assert!(parse_tls_read_reply(&[3, 0, 1, 2, 3], &mut small_out).is_err());
        assert_eq!(small_out, [7; 2]);
    }
}
