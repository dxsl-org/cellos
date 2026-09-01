use alloc::collections::BTreeMap;
use api::ipc::{NetRequest, NetResponse};
use smoltcp::{
    iface::{Interface, SocketSet},
    socket::tcp,
    wire::{IpAddress, IpEndpoint},
};

use super::{make_tcp, send_typed, tcp_state_byte, try_promote};
use crate::{
    service_runtime::next_ephemeral_port,
    socket_state::SocketState,
    socket_table::{SocketOwner, SocketTable},
    tls::socket::TlsSocketEntry,
};

pub(crate) fn handle_tcp_request(
    req: &NetRequest<'_>,
    sender: usize,
    owner: SocketOwner,
    iface: &mut Interface,
    sockets: &mut SocketSet<'_>,
    table: &mut SocketTable,
    tls_table: &mut BTreeMap<u64, TlsSocketEntry>,
) -> bool {
    use NetResponse as R;
    match req {
        NetRequest::TcpConnect { addr, port } => {
            let (handle, cap) = match make_tcp(sockets, table, owner) {
                Ok(t) => t,
                Err(_) => {
                    send_typed(sender, R::Err(0xFF));
                    return true;
                }
            };
            let remote = IpEndpoint::new(IpAddress::v4(addr[0], addr[1], addr[2], addr[3]), *port);
            if sockets
                .get_mut::<tcp::Socket>(handle)
                .connect(iface.context(), remote, next_ephemeral_port())
                .is_err()
            {
                table.remove_internal(cap);
                sockets.remove(handle);
                send_typed(sender, R::Err(0xFF));
                return true;
            }
            table.set_state(cap, SocketState::Connecting);
            send_typed(sender, R::CapId(cap as u32));
            true
        }
        NetRequest::TcpSend { cap_id, data } => {
            let cap = *cap_id as u64;
            if table.is_udp(cap, owner) {
                send_typed(sender, R::Data(&0u32.to_le_bytes()));
                return true;
            }
            try_promote(table, sockets, cap, owner);
            let n = if let Some(h) = table.get(cap, owner) {
                let s = sockets.get_mut::<tcp::Socket>(h);
                if s.can_send() {
                    s.send_slice(data).unwrap_or(0)
                } else {
                    0
                }
            } else {
                0
            };
            send_typed(sender, R::Data(&(n as u32).to_le_bytes()));
            true
        }
        NetRequest::TcpRecv { cap_id, buf_len } => {
            let cap = *cap_id as u64;
            if table.is_udp(cap, owner) {
                send_typed(sender, R::Data(&[]));
                return true;
            }
            try_promote(table, sockets, cap, owner);
            let buf_len = (*buf_len as usize).min(4096);
            let mut data = alloc::vec![0u8; buf_len];
            if let Some(h) = table.get(cap, owner) {
                let s = sockets.get_mut::<tcp::Socket>(h);
                if s.can_recv() {
                    let n = s.recv_slice(&mut data).unwrap_or(0);
                    send_typed(sender, R::Data(&data[..n]));
                } else if !s.may_recv() {
                    send_typed(sender, R::Err(0xFF));
                } else {
                    send_typed(sender, R::Data(&[]));
                }
            } else {
                send_typed(sender, R::Data(&[]));
            }
            true
        }
        NetRequest::TcpClose { cap_id } => {
            let cap = *cap_id as u64;
            if let Some(h) = table.remove(cap, owner) {
                sockets.remove(h);
                tls_table.remove(&cap);
                send_typed(sender, R::Ok);
            } else {
                send_typed(sender, R::Err(0xFF));
            }
            true
        }
        NetRequest::TcpListen { port } => {
            let (handle, cap) = match make_tcp(sockets, table, owner) {
                Ok(t) => t,
                Err(_) => {
                    send_typed(sender, R::Err(0xFF));
                    return true;
                }
            };
            if sockets
                .get_mut::<tcp::Socket>(handle)
                .listen(*port)
                .is_err()
            {
                table.remove_internal(cap);
                sockets.remove(handle);
                send_typed(sender, R::Err(0xFF));
                return true;
            }
            table.set_state(cap, SocketState::Listening);
            table.set_listen_port(cap, *port);
            send_typed(sender, R::CapId(cap as u32));
            true
        }
        NetRequest::TcpAccept { cap_id } => {
            let cap = *cap_id as u64;
            if table.is_udp(cap, owner)
                || table.get_state(cap, owner) != Some(SocketState::Listening)
            {
                send_typed(sender, R::Err(0xFF));
                return true;
            }
            let handle = match table.get(cap, owner) {
                Some(h) => h,
                None => {
                    send_typed(sender, R::Err(0xFF));
                    return true;
                }
            };
            if sockets.get_mut::<tcp::Socket>(handle).state() != tcp::State::Established {
                send_typed(sender, R::Err(0xFE));
                return true;
            }
            let listen_port = match table.get_listen_port(cap, owner) {
                Some(p) => p,
                None => {
                    send_typed(sender, R::Err(0xFF));
                    return true;
                }
            };
            match table.insert_with_state(handle, SocketState::Connected, owner) {
                Ok(stream_cap) => {
                    let mut ns = tcp::Socket::new(
                        tcp::SocketBuffer::new(alloc::vec![0u8; 4096]),
                        tcp::SocketBuffer::new(alloc::vec![0u8; 4096]),
                    );
                    let _ = ns.listen(listen_port);
                    let nh = sockets.add(ns);
                    table.update_handle(cap, nh);
                    table.set_state(cap, SocketState::Listening);
                    table.set_listen_port(cap, listen_port);
                    send_typed(sender, R::CapId(stream_cap as u32));
                }
                Err(_) => {
                    send_typed(sender, R::Err(0xFF));
                }
            }
            true
        }
        NetRequest::SocketState { cap_id } => {
            let cap = *cap_id as u64;
            if table.is_udp(cap, owner) {
                send_typed(sender, R::State(0x00));
                return true;
            }
            let byte = if let Some(h) = table.get(cap, owner) {
                tcp_state_byte(sockets.get_mut::<tcp::Socket>(h).state())
            } else {
                0x00
            };
            send_typed(sender, R::State(byte));
            true
        }
        _ => false,
    }
}
