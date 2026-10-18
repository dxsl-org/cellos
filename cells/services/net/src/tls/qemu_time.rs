//! Explicit QEMU DEV trust boundary: the local SLIRP host is trusted to verify
//! fresh independently signed time and durably retain its floor outside the VM.
//! This is not a protected hardware authority or a general network time client.

use core::sync::atomic::{AtomicU64, Ordering};
use ostd::syscall::sys_get_random;
use crate::service_runtime::sys_get_time;
use smoltcp::{iface::{Interface, SocketHandle, SocketSet}, socket::tcp, wire::{IpAddress, IpEndpoint}};

const SECOND: u64 = 10_000_000;
const MAGIC: &[u8; 8] = b"CTIME001";
static RECEIVED: AtomicU64 = AtomicU64::new(0);
static LOWER: AtomicU64 = AtomicU64::new(0);
static UPPER: AtomicU64 = AtomicU64::new(0);
static UNIX: AtomicU64 = AtomicU64::new(0);
static VALID_UNTIL: AtomicU64 = AtomicU64::new(0);
static DEMAND: AtomicU64 = AtomicU64::new(0);
static FULFILLED: AtomicU64 = AtomicU64::new(0);

/// A restored guest lease is never enough for a new TLS handshake. Require an
/// exchange whose fresh nonce was generated after this handshake requested it.
pub async fn fresh_for_handshake() -> Result<(), embedded_tls::TlsError> {
    let ticket = DEMAND.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
        .map_err(|_| embedded_tls::TlsError::InvalidCertificate)? + 1;
    let deadline = sys_get_time().saturating_add(10 * SECOND);
    core::future::poll_fn(|_| {
        if FULFILLED.load(Ordering::Acquire) >= ticket && validity_window().is_some() {
            core::task::Poll::Ready(Ok(()))
        } else if sys_get_time() >= deadline {
            core::task::Poll::Ready(Err(embedded_tls::TlsError::InvalidCertificate))
        } else {
            core::task::Poll::Pending
        }
    }).await
}

pub fn validity_window() -> Option<(u64, u64)> {
    let deadline = VALID_UNTIL.load(Ordering::Acquire);
    if deadline == 0 { return None; }
    let now = sys_get_time();
    let received = RECEIVED.load(Ordering::Relaxed);
    if now < received || now >= deadline {
        return None;
    }
    let elapsed = now - received;
    let lower = LOWER.load(Ordering::Relaxed).checked_add(elapsed / SECOND)?;
    let upper = UPPER.load(Ordering::Relaxed).checked_add(elapsed.div_ceil(SECOND))?;
    Some((lower, upper))
}

pub fn observe() -> Option<u64> {
    validity_window()?;
    UNIX.load(Ordering::Relaxed).checked_add(
        sys_get_time().checked_sub(RECEIVED.load(Ordering::Relaxed))? / SECOND,
    )
}

struct Exchange {
    handle: SocketHandle,
    request: [u8; 40],
    sent: usize,
    response: [u8; 72],
    received: usize,
    started: u64,
    demand: u64,
}

#[derive(Default)]
pub struct ClockClient {
    exchange: Option<Exchange>,
    next: u64,
    sequence: u64,
    announced: bool,
}

fn decode(response: &[u8; 72], request: &[u8; 40], previous: u64) -> Option<(u64, u64, u64, u64)> {
    if &response[..40] != request {
        return None;
    }
    let unix = u64::from_le_bytes(response[40..48].try_into().ok()?);
    let lower = u64::from_le_bytes(response[48..56].try_into().ok()?);
    let upper = u64::from_le_bytes(response[56..64].try_into().ok()?);
    let sequence = u64::from_le_bytes(response[64..72].try_into().ok()?);
    if lower == 0 || lower > unix || unix > upper || upper.checked_sub(lower)? > 8 || sequence <= previous {
        return None;
    }
    Some((unix, lower, upper, sequence))
}

impl ClockClient {
    /// One bounded step per service turn; no blocking receive or NIC polling.
    pub fn poll(&mut self, iface: &mut Interface, sockets: &mut SocketSet<'_>, configured: bool) {
        let now = sys_get_time();
        if let Some(mut exchange) = self.exchange.take() {
            let socket = sockets.get_mut::<tcp::Socket>(exchange.handle);
            let elapsed = now.checked_sub(exchange.started);
            let mut finished = elapsed.is_none_or(|ticks| ticks > 4 * SECOND);
            if !finished && socket.can_send() && exchange.sent < exchange.request.len() {
                match socket.send_slice(&exchange.request[exchange.sent..]) {
                    Ok(count) => exchange.sent += count,
                    Err(_) => finished = true,
                }
            }
            if !finished && socket.can_recv() {
                match socket.recv_slice(&mut exchange.response[exchange.received..]) {
                    Ok(count) => exchange.received += count,
                    Err(_) => finished = true,
                }
            }
            if exchange.received == exchange.response.len() {
                if !finished {
                    if let Some((mut unix, lower, upper, sequence)) = decode(&exchange.response, &exchange.request, self.sequence) {
                        // The upstream observation happened after this request began.
                        // Include the entire request delay in the upper bound.
                        let delay = elapsed.unwrap().div_ceil(SECOND);
                        if let Some(upper) = upper.checked_add(delay) {
                            unix = unix.max(observe().unwrap_or(0));
                            if unix <= upper {
                                VALID_UNTIL.store(0, Ordering::Release);
                                LOWER.store(lower, Ordering::Relaxed);
                                UPPER.store(upper, Ordering::Relaxed);
                                UNIX.store(unix, Ordering::Relaxed);
                                RECEIVED.store(now, Ordering::Relaxed);
                                VALID_UNTIL.store(now.saturating_add(2 * SECOND), Ordering::Release);
                                self.sequence = sequence;
                                FULFILLED.store(exchange.demand, Ordering::Release);
                                if !self.announced {
                                    ostd::io::println(&alloc::format!("[net/time] QEMU DEV authenticated time ready: unix={} sequence={} source=time.txryan.com; trusted host floor", unix, sequence));
                                    self.announced = true;
                                }
                            }
                        }
                    }
                }
                finished = true;
            }
            if matches!(socket.state(), tcp::State::Closed | tcp::State::CloseWait) {
                finished = true;
            }
            if finished {
                socket.abort();
                sockets.remove(exchange.handle);
                self.next = now.saturating_add(SECOND / 2);
            } else {
                self.exchange = Some(exchange);
            }
            return;
        }
        if !configured || now < self.next {
            return;
        }
        self.next = now.saturating_add(SECOND);
        let mut request = [0u8; 40];
        request[..8].copy_from_slice(MAGIC);
        let mut filled = 8;
        // A short entropy read is allowed; missing entropy never produces a request.
        for _ in 0..8 {
            if filled == request.len() { break; }
            let count = sys_get_random(&mut request[filled..]);
            if count == 0 || count > request.len() - filled { return; }
            filled += count;
        }
        if filled != request.len() { return; }
        let handle = sockets.add(tcp::Socket::new(
            tcp::SocketBuffer::new(alloc::vec![0u8; 128]),
            tcp::SocketBuffer::new(alloc::vec![0u8; 128]),
        ));
        let remote = IpEndpoint::new(IpAddress::v4(10, 0, 2, 2), 18443);
        if sockets.get_mut::<tcp::Socket>(handle).connect(iface.context(), remote, crate::service_runtime::next_ephemeral_port()).is_err() {
            sockets.remove(handle);
            return;
        }
        self.exchange = Some(Exchange { handle, request, sent: 0, response: [0; 72], received: 0, started: now, demand: DEMAND.load(Ordering::Acquire) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_must_bind_nonce_sequence_and_bounded_interval() {
        let mut request = [7u8; 40];
        request[..8].copy_from_slice(MAGIC);
        let mut reply = [0u8; 72];
        reply[..40].copy_from_slice(&request);
        reply[40..48].copy_from_slice(&100u64.to_le_bytes());
        reply[48..56].copy_from_slice(&98u64.to_le_bytes());
        reply[56..64].copy_from_slice(&102u64.to_le_bytes());
        reply[64..72].copy_from_slice(&5u64.to_le_bytes());
        assert_eq!(decode(&reply, &request, 4), Some((100, 98, 102, 5)));
        assert_eq!(decode(&reply, &request, 5), None);
        reply[8] ^= 1;
        assert_eq!(decode(&reply, &request, 4), None);
        reply[8] ^= 1;
        reply[56..64].copy_from_slice(&107u64.to_le_bytes());
        assert_eq!(decode(&reply, &request, 4), None);
        reply[56..64].copy_from_slice(&99u64.to_le_bytes());
        assert_eq!(decode(&reply, &request, 4), None);
    }
}
