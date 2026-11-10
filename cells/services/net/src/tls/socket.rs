//! Owned async TLS state for each raw TLS socket.
//!
//! The record buffers are owned alongside the connection, not leaked on close,
//! timeout, or owner death. The connection is always dropped before its buffers.
//!
//! `embedded-tls` needs the buffers as `&'static mut [u8]`. They are therefore
//! held as raw heap pointers, never as `Box`es: moving a `Box` asserts unique
//! ownership of its allocation, which under Stacked/Tree Borrows invalidates
//! every slice derived from it earlier. A raw pointer makes no such claim, so
//! the entry (and the futures that carry it) may move while `conn` uses them.

extern crate alloc;

use crate::tls::rng::ViRng;
use crate::tls::transport::SmoltcpTlsTransport;
use alloc::boxed::Box;
use alloc::string::String;
use embedded_tls::{Aes128GcmSha256, TlsConfig, TlsConnection, TlsContext, TlsError};
use smoltcp::iface::SocketHandle;

#[cfg(any(feature = "tls-roots-embedded", feature = "qemu-dev-https"))]
use crate::tls::provider::ViTlsProvider;
#[cfg(feature = "tls-insecure")]
use embedded_tls::UnsecureProvider;

#[cfg(not(any(
    feature = "tls-roots-embedded",
    feature = "qemu-dev-https",
    feature = "tls-insecure",
)))]
compile_error!(
    "service-net: select tls-roots-embedded (fleet), qemu-dev-https (public DEV), or tls-insecure."
);

const TLS_BUF: usize = 16640;

/// The two TLS record buffers a connection borrows, freed when this is dropped.
///
/// Invariant: each pointer came from `Box::into_raw` of a `Box<[u8; TLS_BUF]>`,
/// is owned solely by this value, and is freed exactly once, in `Drop`.
pub(super) struct RecordBuffers {
    read: *mut [u8; TLS_BUF],
    write: *mut [u8; TLS_BUF],
}

impl RecordBuffers {
    fn new() -> Self {
        Self {
            read: Box::into_raw(Box::new([0u8; TLS_BUF])),
            write: Box::into_raw(Box::new([0u8; TLS_BUF])),
        }
    }

    /// The `(read, write)` record slices for one `TlsConnection`.
    ///
    /// # Safety
    /// Call at most once per value (the slices are exclusive), and drop every
    /// returned slice — i.e. the connection that borrows them — before `self`.
    unsafe fn record_slices(&mut self) -> (&'static mut [u8], &'static mut [u8]) {
        // SAFETY: both pointers are live, distinct, unaliased allocations (type
        // invariant). The caller takes them once and ends their use before
        // `Drop` frees the allocations; moving `self` meanwhile copies only the
        // raw pointers and leaves the derived slices valid.
        unsafe { (&mut *self.read, &mut *self.write) }
    }
}

impl Drop for RecordBuffers {
    fn drop(&mut self) {
        // SAFETY: each pointer is the unique `Box::into_raw` result of its
        // allocation and is freed only here, once; every slice derived from it
        // belonged to a connection that is dropped first.
        unsafe {
            drop(Box::from_raw(self.read));
            drop(Box::from_raw(self.write));
        }
    }
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
    Ok(RecordBuffers::new())
}

pub struct TlsSocketEntry {
    // Field order matters: drop the connection (and its crypto keys) before
    // releasing the backing buffers it borrows.
    conn: TlsConnection<'static, SmoltcpTlsTransport, Aes128GcmSha256>,
    _buffers: RecordBuffers,
}

impl TlsSocketEntry {
    /// Consumes the hostname and owns all resources across suspended polls.
    pub async fn handshake(
        handle: SocketHandle,
        hostname: String,
        time_ticket: (u64, u64),
    ) -> Result<Self, TlsError> {
        let started = crate::service_runtime::sys_get_time();
        ostd::io::println(&alloc::format!(
            "[net/tls] Starting handshake for hostname={}",
            hostname
        ));
        #[cfg(not(feature = "qemu-dev-https"))]
        let _ = time_ticket;
        #[cfg(feature = "qemu-dev-https")]
        if let Err(e) = super::qemu_time::fresh_for_ticket(time_ticket.0, time_ticket.1).await {
            ostd::io::println(&alloc::format!(
                "[net/tls] fresh_for_ticket failed: {:?}",
                e
            ));
            return Err(e);
        }
        let time_ready = crate::service_runtime::sys_get_time();
        let mut buffers = match prepare_handshake_buffers() {
            Ok(b) => b,
            Err(e) => {
                ostd::io::println(&alloc::format!(
                    "[net/tls] prepare_handshake_buffers failed: {:?}",
                    e
                ));
                return Err(e);
            }
        };
        // SAFETY: the slices are taken once from these buffers. `conn` is
        // destroyed before `buffers` on every path: as a later local here
        // (including a future dropped while suspended) and as the earlier
        // field of the completed entry.
        let (read, write) = unsafe { buffers.record_slices() };
        let transport = SmoltcpTlsTransport::new(handle);
        let mut conn = TlsConnection::new(transport, read, write);
        let config = if hostname.is_empty() {
            TlsConfig::new()
        } else {
            TlsConfig::new().with_server_name(&hostname)
        };

        #[cfg(any(feature = "tls-roots-embedded", feature = "qemu-dev-https"))]
        {
            if hostname.is_empty() {
                return Err(TlsError::InvalidCertificate);
            }
            let provider = match ViTlsProvider::new(ViRng::new(), &hostname) {
                Ok(p) => p,
                Err(e) => {
                    ostd::io::println(&alloc::format!(
                        "[net/tls] ViTlsProvider::new failed: {:?}",
                        e
                    ));
                    return Err(e);
                }
            };
            if let Err(e) = conn.open(TlsContext::new(&config, provider)).await {
                ostd::io::println(&alloc::format!("[net/tls] conn.open failed: {:?}", e));
                return Err(e);
            }
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

        let done = crate::service_runtime::sys_get_time();
        ostd::io::println(&alloc::format!(
            "[net/tls] handshake {} timing: time-gate {}ms, tls {}ms",
            hostname,
            time_ready.saturating_sub(started) / 10_000,
            done.saturating_sub(time_ready) / 10_000
        ));
        Ok(Self {
            conn,
            _buffers: buffers,
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
