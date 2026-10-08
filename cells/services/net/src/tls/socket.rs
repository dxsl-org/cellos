//! Owned async TLS state for each raw TLS socket.
//!
//! The record buffers are owned alongside the connection, not leaked on close,
//! timeout, or owner death. The connection is always dropped before its buffers.

extern crate alloc;

use crate::tls::rng::ViRng;
use crate::tls::transport::SmoltcpTlsTransport;
use alloc::boxed::Box;
use alloc::string::String;
use embedded_tls::{Aes128GcmSha256, TlsConfig, TlsConnection, TlsContext, TlsError};
use smoltcp::iface::SocketHandle;

#[cfg(feature = "tls-roots-embedded")]
use crate::tls::provider::ViTlsProvider;
#[cfg(feature = "tls-insecure")]
use embedded_tls::UnsecureProvider;

#[cfg(not(any(feature = "tls-roots-embedded", feature = "tls-insecure")))]
compile_error!(
    "service-net: select a TLS flavor — `tls-roots-embedded` (verifying) or `tls-insecure` (dev only). \
     `tls-roots-full` is not yet implemented (see plan phase-04)."
);

const TLS_BUF: usize = 16640;

pub(super) struct RecordBuffers {
    read: Box<[u8; TLS_BUF]>,
    write: Box<[u8; TLS_BUF]>,
}

fn authenticated_time_preflight() -> Result<(), TlsError> {
    crate::tls::clock::observe()
        .map(|_| ())
        .ok_or(TlsError::InvalidCertificate)
}

pub(super) fn prepare_handshake_buffers() -> Result<RecordBuffers, TlsError> {
    authenticated_time_preflight()?;
    #[cfg(test)]
    super::authenticated_time_precheck_tests::record_buffer_allocation();
    let read = Box::new([0u8; TLS_BUF]);
    let write = Box::new([0u8; TLS_BUF]);
    Ok(RecordBuffers { read, write })
}

pub struct TlsSocketEntry {
    // Field order matters: drop the connection (and its crypto keys) before
    // releasing the backing buffers it borrows.
    conn: TlsConnection<'static, SmoltcpTlsTransport, Aes128GcmSha256>,
    _buffers: RecordBuffers,
    pub handle: SocketHandle,
}

impl TlsSocketEntry {
    /// Consumes the hostname and owns all resources across suspended polls.
    pub async fn handshake(handle: SocketHandle, hostname: String) -> Result<Self, TlsError> {
        let mut buffers = prepare_handshake_buffers()?;
        // SAFETY: both allocations live at stable heap addresses. `conn` is
        // destroyed before `buffers` on every path (including future drop while
        // suspended); the completed entry likewise declares conn first.
        let read = unsafe { core::slice::from_raw_parts_mut(buffers.read.as_mut_ptr(), TLS_BUF) };
        let write = unsafe { core::slice::from_raw_parts_mut(buffers.write.as_mut_ptr(), TLS_BUF) };
        let transport = SmoltcpTlsTransport::new(handle);
        let mut conn = TlsConnection::new(transport, read, write);
        let config = if hostname.is_empty() {
            TlsConfig::new()
        } else {
            TlsConfig::new().with_server_name(&hostname)
        };

        #[cfg(feature = "tls-roots-embedded")]
        {
            if hostname.is_empty() {
                return Err(TlsError::InvalidCertificate);
            }
            let provider = ViTlsProvider::new(ViRng::new(), &hostname)?;
            conn.open(TlsContext::new(&config, provider)).await?;
        }

        #[cfg(feature = "tls-insecure")]
        {
            static BANNER_PRINTED: core::sync::atomic::AtomicBool =
                core::sync::atomic::AtomicBool::new(false);
            if !BANNER_PRINTED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                let _ = ostd::syscall::sys_log(
                    "[net/tls] !!! INSECURE TLS BUILD - server certs NOT verified !!!",
                );
            }
            let provider = UnsecureProvider::new::<Aes128GcmSha256>(ViRng::new());
            conn.open(TlsContext::new(&config, provider)).await?;
        }

        Ok(Self {
            conn,
            _buffers: buffers,
            handle,
        })
    }

    pub async fn send(&mut self, data: &[u8]) -> Result<usize, TlsError> {
        let written = self.conn.write(data).await?;
        self.conn.flush().await?;
        Ok(written)
    }

    pub async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, TlsError> {
        self.conn.read(buf).await
    }
}
