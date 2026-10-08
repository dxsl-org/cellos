//! Raw TLS IPC dispatch for the net service cell.

extern crate alloc;
use crate::{
    interface::VirtioNetDevice,
    service_runtime::next_ephemeral_port,
    socket_state::SocketState,
    socket_table::{SocketOwner, SocketTable},
    tls::dispatch::{ReplyTo, Request},
    tls::socket::TlsSocketEntry,
    tls_wire::{parse_raw_tls_request, RawTlsRequest},
};
pub use crate::tls::dispatch::TlsPending;
use alloc::collections::BTreeMap;
use alloc::string::ToString;
use ostd::io::println;
use ostd::syscall::sys_send;
use smoltcp::{
    iface::{Interface, SocketHandle, SocketSet},
    socket::tcp,
    wire::{IpAddress, IpEndpoint},
};
fn authenticated_time_available() -> bool {
    crate::tls::clock::observe().is_some()
}

/// Announce the fail-closed refusal once.
///
/// Without authenticated time every `TLS_CONNECT` is refused before any
/// certificate is seen, so the caller only observes a zero cap ("handshake
/// failed") and cannot tell a policy refusal from a transport fault. One named
/// line of evidence removes that ambiguity — and gives the `tls-gate` lane
/// something real to assert instead of an unreachable reject string.
fn announce_tls_refusal_once() {
    static ANNOUNCED: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);
    if !ANNOUNCED.swap(true, core::sync::atomic::Ordering::Relaxed) {
        println("[net/tls] TLS connect refused: authenticated time unavailable (fail-closed)");
    }
}
fn send_connect_reply(sender: usize, cap: u64) {
    let reply = cap.to_le_bytes();
    #[cfg(test)]
    crate::tls::authenticated_time_precheck_tests::record_connect_reply(&reply);
    sys_send(sender, &reply);
}
fn make_tcp(
    sockets: &mut SocketSet<'_>,
    table: &mut SocketTable,
    owner: SocketOwner,
) -> Result<(SocketHandle, u64), ()> {
    if !table.can_insert(owner) {
        return Err(());
    }
    let handle = sockets.add(tcp::Socket::new(
        tcp::SocketBuffer::new(alloc::vec![0u8; 4096]),
        tcp::SocketBuffer::new(alloc::vec![0u8; 4096]),
    ));
    match table.insert(handle, owner) {
        Ok(cap) => Ok((handle, cap)),
        Err(_) => {
            sockets.remove(handle);
            Err(())
        }
    }
}
/// Admit a raw TLS request without waiting for peer I/O. Deferred requests
/// reply only when the original operation completes.
#[allow(clippy::too_many_arguments)]
pub fn handle_tls_raw(
    buf: &[u8],
    sender: usize,
    owner: SocketOwner,
    iface: &mut Interface,
    _device: &mut VirtioNetDevice,
    sockets: &mut SocketSet<'_>,
    table: &mut SocketTable,
    tls_table: &mut BTreeMap<u64, TlsSocketEntry>,
    pending: &mut TlsPending,
) {
    let req = match parse_raw_tls_request(buf) {
        Ok(r) => r,
        Err(_) => {
            sys_send(sender, &[]);
            return;
        }
    };
    match req {
        RawTlsRequest::Close { cap } => {
            if let Some(handle) = table.remove(cap, owner) {
                pending.cancel_cap(cap);
                tls_table.remove(&cap);
                sockets.remove(handle);
                sys_send(sender, &[0x00]);
            } else {
                sys_send(sender, &[0xFF]);
            }
        }
        RawTlsRequest::Connect { addr, port, hostname } => {
            if !authenticated_time_available() {
                announce_tls_refusal_once();
                send_connect_reply(sender, 0);
                return;
            }
            // Async callers use the exact operation token. Legacy blocking
            // clients use their sender TID and a deferred nonblocking reply.
            let reply = ReplyTo::capture(sender);
            let (handle, cap) = match make_tcp(sockets, table, owner) {
                Ok(pair) => pair,
                Err(_) => {
                    send_connect_reply(sender, 0);
                    return;
                }
            };
            let remote = IpEndpoint::new(IpAddress::v4(addr[0], addr[1], addr[2], addr[3]), port);
            if sockets.get_mut::<tcp::Socket>(handle)
                .connect(iface.context(), remote, next_ephemeral_port()).is_err()
            {
                table.remove_internal(cap);
                sockets.remove(handle);
                send_connect_reply(sender, 0);
                return;
            }
            table.set_state(cap, SocketState::Connecting);
            pending.connect(cap, handle, owner, reply, hostname.to_string());
        }
        RawTlsRequest::Send { cap, data } => {
            if !table.is_owner(cap, owner) {
                sys_send(sender, &0u32.to_le_bytes());
                return;
            }
            pending.enqueue(cap, owner, ReplyTo::capture(sender),
                Request::Send(data.to_vec()), tls_table, table);
        }
        RawTlsRequest::Recv { cap, buf_len } => {
            if !table.is_owner(cap, owner) {
                sys_send(sender, &[0u8; 2]);
                return;
            }
            pending.enqueue(cap, owner, ReplyTo::capture(sender),
                Request::Recv(buf_len), tls_table, table);
        }
    }
}
