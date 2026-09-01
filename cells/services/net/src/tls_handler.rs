//! Raw TLS IPC dispatch for the net service cell.

extern crate alloc;

use crate::{
    interface::VirtioNetDevice,
    service_runtime::{next_ephemeral_port, now_instant},
    socket_state::SocketState,
    socket_table::{SocketOwner, SocketTable},
    tls::socket::TlsSocketEntry,
    tls_wire::{encode_tls_recv_reply, parse_raw_tls_request, RawTlsRequest},
};
use alloc::collections::BTreeMap;
use ostd::syscall::{sys_get_time, sys_heartbeat, sys_send};
use smoltcp::{
    iface::{Interface, SocketHandle, SocketSet},
    socket::tcp,
    wire::{IpAddress, IpEndpoint},
};
fn authenticated_time_available() -> bool {
    crate::tls::clock::observe().is_some()
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
#[allow(clippy::too_many_arguments)]
pub fn handle_tls_raw(
    buf: &[u8],
    sender: usize,
    owner: SocketOwner,
    iface: &mut Interface,
    device: &mut VirtioNetDevice,
    sockets: &mut SocketSet<'_>,
    table: &mut SocketTable,
    tls_table: &mut BTreeMap<u64, TlsSocketEntry>,
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
            if let Some(h) = table.remove(cap, owner) {
                sockets.remove(h);
                tls_table.remove(&cap);
                sys_send(sender, &[0x00]);
            } else {
                sys_send(sender, &[0xFF]);
            }
        }
        RawTlsRequest::Connect {
            addr,
            port,
            hostname,
        } => {
            if !authenticated_time_available() {
                send_connect_reply(sender, 0);
                return;
            }
            let (handle, cap_id) = match make_tcp(sockets, table, owner) {
                Ok(t) => t,
                Err(_) => {
                    send_connect_reply(sender, 0);
                    return;
                }
            };
            let remote = IpEndpoint::new(IpAddress::v4(addr[0], addr[1], addr[2], addr[3]), port);
            if sockets
                .get_mut::<tcp::Socket>(handle)
                .connect(iface.context(), remote, next_ephemeral_port())
                .is_err()
            {
                table.remove_internal(cap_id);
                sockets.remove(handle);
                send_connect_reply(sender, 0);
                return;
            }
            table.set_state(cap_id, SocketState::Connecting);

            let tcp_deadline = sys_get_time() + 150_000_000;
            let mut next_hb = sys_get_time() + 5_000_000;
            loop {
                device.pump_rx();
                iface.poll(now_instant(), device, sockets);
                match sockets.get_mut::<tcp::Socket>(handle).state() {
                    tcp::State::Established => break,
                    tcp::State::Closed | tcp::State::CloseWait => {
                        table.remove_internal(cap_id);
                        sockets.remove(handle);
                        send_connect_reply(sender, 0);
                        return;
                    }
                    _ => {}
                }
                let now = sys_get_time();
                if now >= tcp_deadline {
                    table.remove_internal(cap_id);
                    sockets.remove(handle);
                    send_connect_reply(sender, 0);
                    return;
                }
                if now >= next_hb {
                    sys_heartbeat(500);
                    next_hb = now + 5_000_000;
                }
                core::hint::spin_loop();
            }
            table.set_state(cap_id, SocketState::Connected);

            let sockets_ptr = sockets as *mut SocketSet<'_> as *mut ();
            unsafe {
                crate::tls::transport::set_tls_context(
                    iface as *mut Interface,
                    device as *mut VirtioNetDevice,
                    sockets_ptr,
                );
            }
            match unsafe { TlsSocketEntry::handshake(handle, hostname) } {
                Ok(entry) => {
                    tls_table.insert(cap_id, entry);
                    send_connect_reply(sender, cap_id);
                }
                Err(_) => {
                    table.remove_internal(cap_id);
                    sockets.remove(handle);
                    send_connect_reply(sender, 0);
                }
            }
        }
        RawTlsRequest::Send { cap, data } => {
            if !table.is_owner(cap, owner) {
                sys_send(sender, &0u32.to_le_bytes());
                return;
            }
            let sockets_ptr = sockets as *mut SocketSet<'_> as *mut ();
            let result = tls_table.get_mut(&cap).map(|entry| unsafe {
                entry.send(
                    data,
                    iface as *mut Interface,
                    device as *mut VirtioNetDevice,
                    sockets_ptr,
                )
            });
            match result {
                Some(Ok(n)) => sys_send(sender, &(n as u32).to_le_bytes()),
                _ => sys_send(sender, &0u32.to_le_bytes()),
            };
        }
        RawTlsRequest::Recv { cap, buf_len } => {
            if !table.is_owner(cap, owner) {
                sys_send(sender, &[0u8; 2]);
                return;
            }
            let sockets_ptr = sockets as *mut SocketSet<'_> as *mut ();
            let result = tls_table.get_mut(&cap).map(|entry| {
                let mut data = alloc::vec![0u8; buf_len];
                let r = unsafe {
                    entry.recv(
                        &mut data,
                        iface as *mut Interface,
                        device as *mut VirtioNetDevice,
                        sockets_ptr,
                    )
                };
                (data, r)
            });
            match result {
                Some((data, Ok(n))) => {
                    let resp = encode_tls_recv_reply(&data[..n]);
                    sys_send(sender, &resp);
                }
                _ => {
                    sys_send(sender, &[0u8; 2]);
                }
            }
        }
    }
}
