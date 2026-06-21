// SPDX-License-Identifier: MPL-2.0

//! `TlsStream` — an [`embedded_io`] adapter over the raw `ostd::tls` IPC.
//!
//! Mirrors [`TcpStream`][super::net::TcpStream]: it turns the net cell's
//! request/reply TLS opcodes ([`tls_connect`]/[`tls_write`]/[`tls_read`]/
//! [`tls_close`]) into a blocking `Read + Write` stream so a generic
//! [`HttpClient`][crate::http::HttpClient] can drive HTTPS exactly the same way
//! it drives plaintext HTTP over `TcpStream`.
//!
//! # Blocking / IPC contract
//! - [`connect`][TlsStream::connect] resolves the NET service TID, then performs
//!   the TCP + TLS handshake synchronously inside the net cell. It returns only
//!   after the handshake completes (or the net cell reports failure).
//! - [`Write::write`] loops over the 503-byte-per-call cap of [`tls_write`]
//!   until every byte of `buf` is accepted (write-all semantics), because a
//!   single TLS record write may be partially accepted by the net cell.
//! - [`Read::read`] spin-yields while [`tls_read`] returns 0. A 0-read over TLS
//!   is ambiguous: it means EITHER "no application data buffered yet" OR
//!   "peer closed" (net `handlers.rs:462`) — there is no EOF signal. Body
//!   completion is therefore decided by HTTP framing in
//!   [`BodyReader`][crate::http::BodyReader], NOT by a 0-read here. To stop a
//!   genuinely dead connection from hanging forever, the spin is bounded by a
//!   retry budget; exhausting it surfaces as [`ViError::WouldBlock`] (mapped to
//!   `embedded_io::ErrorKind::Other`) rather than an unbounded loop.
//!
//! # Security posture
//! `TlsStream` inherits the net cell's current TLS verifier — today that is
//! embedded-tls `UnsecureProvider` with **no certificate verification**.
//! `HttpClient<TlsStream>` is therefore only as trustworthy as the net cell;
//! hardening cert verification is a separate net-cell workstream. Do not treat
//! this as authenticated HTTPS.

extern crate alloc;

use crate::service;
use crate::tls::{tls_close, tls_connect, tls_read, tls_write};
use crate::{ViError, ViResult};

/// Max bytes accepted by a single [`tls_write`] IPC call (mirrors the cap in
/// `tls.rs`: 512-byte IPC buffer minus the 9-byte `[op][cap]` header).
const MAX_TLS_WRITE: usize = 503;

/// Number of consecutive 0-byte reads tolerated before [`Read::read`] gives up.
///
/// Each miss yields the scheduler, so this is a "no progress for N scheduler
/// turns" budget, not a wall-clock timeout (ostd has no per-read timer here).
/// Sized generously so a slow-but-live LLM response never trips it, while a
/// mid-body peer drop still terminates instead of hanging. A caller that wants
/// a stricter bound layers its own timeout above the stream.
const READ_RETRY_BUDGET: u32 = 100_000;

/// A TLS 1.3 connection handle implementing [`embedded_io::Read`] +
/// [`embedded_io::Write`].
///
/// The underlying connection (and its TCP socket) is closed when the handle is
/// dropped (RAII — Law 8).
pub struct TlsStream {
    net_tid: usize,
    cap_id: u64,
}

impl TlsStream {
    /// Open a TLS 1.3 connection to `addr:port` with SNI `hostname`.
    ///
    /// Resolves the NET service via [`service::lookup`] and performs the
    /// handshake synchronously. Returns `ViError::NotFound` if the net service
    /// is unavailable, or `ViError::IO` if the handshake fails.
    pub fn connect(addr: [u8; 4], port: u16, hostname: &str) -> ViResult<Self> {
        let net_tid = service::lookup(service::service::NET).ok_or(ViError::NotFound)?;
        let cap_id = tls_connect(net_tid, addr, port, hostname);
        if cap_id == 0 {
            return Err(ViError::IO);
        }
        Ok(Self { net_tid, cap_id })
    }
}

impl Drop for TlsStream {
    fn drop(&mut self) {
        tls_close(self.net_tid, self.cap_id);
    }
}

impl embedded_io::ErrorType for TlsStream {
    type Error = crate::io::OstdError;
}

impl embedded_io::Read for TlsStream {
    /// Block (with cooperative yields) until at least one byte is available.
    ///
    /// A 0-read from the net cell is treated as "not ready yet" and retried, up
    /// to [`READ_RETRY_BUDGET`] times. The HTTP layer never relies on a 0-read
    /// to mean EOF — see the module doc.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, crate::io::OstdError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut misses: u32 = 0;
        loop {
            let n = tls_read(self.net_tid, self.cap_id, buf);
            if n > 0 {
                return Ok(n);
            }
            misses += 1;
            if misses >= READ_RETRY_BUDGET {
                // Dead connection or stalled peer — surface as WouldBlock rather
                // than spinning forever (EOF-less TLS guard, plan risk #2).
                return Err(crate::io::OstdError(ViError::WouldBlock));
            }
            crate::task::yield_now();
        }
    }
}

impl embedded_io::Write for TlsStream {
    /// Write-all: loop over the 503-byte [`tls_write`] cap until every byte of
    /// `buf` has been accepted by the net cell.
    ///
    /// Returns `Ok(buf.len())` on success. A stalled send (the net cell accepts
    /// 0 bytes) surfaces as `ViError::WouldBlock` after the retry budget,
    /// rather than looping forever.
    fn write(&mut self, buf: &[u8]) -> Result<usize, crate::io::OstdError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut sent = 0usize;
        let mut stalls: u32 = 0;
        while sent < buf.len() {
            let chunk_end = (sent + MAX_TLS_WRITE).min(buf.len());
            let n = tls_write(self.net_tid, self.cap_id, &buf[sent..chunk_end]);
            if n == 0 {
                stalls += 1;
                if stalls >= READ_RETRY_BUDGET {
                    return Err(crate::io::OstdError(ViError::WouldBlock));
                }
                crate::task::yield_now();
                continue;
            }
            stalls = 0;
            sent += n;
        }
        Ok(sent)
    }

    fn flush(&mut self) -> Result<(), crate::io::OstdError> {
        Ok(()) // tls_write commits synchronously through the net cell
    }
}
