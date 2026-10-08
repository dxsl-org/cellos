//! Nonblocking smoltcp transport used by the async TLS connection.
//!
//! Only one net service turn runs at a time. The dispatcher installs the three
//! borrowed smoltcp objects immediately before polling a TLS future and clears
//! them again before returning to IPC dispatch. A transport operation that cannot
//! proceed returns Pending; the service loop, not a transport spin loop, polls it
//! again on the next turn.

use crate::interface::VirtioNetDevice;
use core::future::poll_fn;
use core::sync::atomic::{AtomicPtr, Ordering};
use core::task::Poll;
use embedded_io::{ErrorKind, ErrorType};
use embedded_io_async::{Read, Write};
use smoltcp::{
    iface::{Interface, SocketHandle, SocketSet},
    socket::tcp,
};

static TLS_IFACE: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static TLS_DEVICE: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static TLS_SOCKETS: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// Install borrowed context for the duration of ONE synchronous Future::poll.
///
/// # Safety
/// All pointers must remain valid, exclusive, and point at the same socket set
/// until `clear_tls_context` is called. No future may be polled after its socket
/// has been removed.
pub unsafe fn set_tls_context(iface: *mut Interface, device: *mut VirtioNetDevice, sockets: *mut ()) {
    TLS_IFACE.store(iface.cast(), Ordering::Relaxed);
    TLS_DEVICE.store(device.cast(), Ordering::Relaxed);
    TLS_SOCKETS.store(sockets, Ordering::Relaxed);
}

/// Remove borrowed pointers before the dispatcher may mutate the socket set.
pub fn clear_tls_context() {
    TLS_IFACE.store(core::ptr::null_mut(), Ordering::Relaxed);
    TLS_DEVICE.store(core::ptr::null_mut(), Ordering::Relaxed);
    TLS_SOCKETS.store(core::ptr::null_mut(), Ordering::Relaxed);
}

pub struct SmoltcpTlsTransport {
    pub handle: SocketHandle,
}

impl SmoltcpTlsTransport {
    pub fn new(handle: SocketHandle) -> Self {
        Self { handle }
    }

    unsafe fn sockets() -> &'static mut SocketSet<'static> {
        &mut *(TLS_SOCKETS.load(Ordering::Relaxed) as *mut SocketSet<'static>)
    }

    unsafe fn poll_network() {
        let iface = &mut *(TLS_IFACE.load(Ordering::Relaxed) as *mut Interface);
        let device = &mut *(TLS_DEVICE.load(Ordering::Relaxed) as *mut VirtioNetDevice);
        let sockets = Self::sockets();
        iface.poll(crate::service_runtime::now_instant(), device, sockets);
    }
}

impl ErrorType for SmoltcpTlsTransport {
    type Error = ErrorKind;
}

impl Read for SmoltcpTlsTransport {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ErrorKind> {
        if buf.is_empty() {
            return Ok(0);
        }
        poll_fn(|_| {
            // SAFETY: the dispatcher installs a valid context for this poll.
            let socket = unsafe { Self::sockets().get_mut::<tcp::Socket>(self.handle) };
            if socket.can_recv() {
                return Poll::Ready(socket.recv_slice(buf).map_err(|_| ErrorKind::Other));
            }
            if !socket.may_recv() {
                return Poll::Ready(Err(ErrorKind::ConnectionReset));
            }
            Poll::Pending
        })
        .await
    }
}

impl Write for SmoltcpTlsTransport {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, ErrorKind> {
        if buf.is_empty() {
            return Ok(0);
        }
        poll_fn(|_| {
            let socket = unsafe { Self::sockets().get_mut::<tcp::Socket>(self.handle) };
            if socket.can_send() {
                let n = socket.send_slice(buf).map_err(|_| ErrorKind::Other)?;
                // Promptly transmit a record; never wait for the remote ACK here.
                unsafe { Self::poll_network() };
                return Poll::Ready(Ok(n));
            }
            if !socket.may_send() {
                return Poll::Ready(Err(ErrorKind::BrokenPipe));
            }
            Poll::Pending
        })
        .await
    }

    async fn flush(&mut self) -> Result<(), ErrorKind> {
        unsafe { Self::poll_network() };
        Ok(())
    }
}
