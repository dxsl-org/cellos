use ostd::ipc::reply_to;
use smoltcp::{
    iface::{Interface, SocketHandle, SocketSet},
    socket::{udp, Socket},
    wire::{IpAddress, IpEndpoint},
};
use wire::ipc::{self, NetRequest, NetResponse, IPC_BUF_SIZE};
use wire::proto::net::NetError;

use super::send_typed;
use crate::{
    interface::VirtioNetDevice,
    service_runtime::now_instant,
    socket_state::SocketState,
    socket_table::{SocketOwner, SocketTable},
    stack_entropy::next_ephemeral_port,
};

/// Cursor draws an unbound-port search may spend before it gives up. Every draw
/// advances the cursor by at least one port, so this covers over a thousand
/// distinct ports of the dynamic range.
const EPHEMERAL_PORT_DRAWS: usize = 1024;

/// A port from `draw` (the service's dynamic-range cursor, never a well-known
/// port) that no UDP socket in `sockets` is bound to, or `None` when every draw
/// was taken.
pub(super) fn unbound_udp_port(
    sockets: &SocketSet<'_>,
    mut draw: impl FnMut() -> u16,
) -> Option<u16> {
    (0..EPHEMERAL_PORT_DRAWS).map(|_| draw()).find(|&port| {
        !sockets
            .iter()
            .any(|(_, socket)| matches!(socket, Socket::Udp(s) if s.endpoint().port == port))
    })
}

/// Standard UDP client behaviour (annex S2-N, amendment 2026-11-10): the first
/// send on an unbound socket binds it to an unused ephemeral port, so replies to
/// that port reach this socket. This needs only the Connect right `UdpSend` was
/// admitted with; choosing a port, a well-known one included, stays the
/// Listen-gated `UdpBind`.
fn ensure_client_bound(sockets: &mut SocketSet<'_>, handle: SocketHandle) -> bool {
    if sockets.get::<udp::Socket>(handle).is_open() {
        return true;
    }
    unbound_udp_port(sockets, next_ephemeral_port)
        .is_some_and(|port| sockets.get_mut::<udp::Socket>(handle).bind(port).is_ok())
}

pub(crate) fn handle_udp_request(
    req: &NetRequest<'_>,
    sender: usize,
    owner: SocketOwner,
    iface: &mut Interface,
    device: &mut VirtioNetDevice,
    sockets: &mut SocketSet<'_>,
    table: &mut SocketTable,
) -> bool {
    use NetResponse as R;
    match req {
        NetRequest::UdpCreate => {
            if !table.can_insert(owner) {
                send_typed(sender, R::Err(NetError::Failure as u8));
                return true;
            }
            let handle = sockets.add(udp::Socket::new(
                udp::PacketBuffer::new(
                    alloc::vec![udp::PacketMetadata::EMPTY; 4],
                    alloc::vec![0u8; 1024],
                ),
                udp::PacketBuffer::new(
                    alloc::vec![udp::PacketMetadata::EMPTY; 4],
                    alloc::vec![0u8; 1024],
                ),
            ));
            match table.insert(handle, owner) {
                Ok(cap) => {
                    table.mark_udp(cap);
                    send_typed(sender, R::CapId(cap as u32));
                }
                Err(_) => {
                    sockets.remove(handle);
                    send_typed(sender, R::Err(NetError::Failure as u8));
                }
            }
            true
        }
        NetRequest::UdpBind { cap_id, port } => {
            let cap = *cap_id as u64;
            let port = if *port == 0 {
                unbound_udp_port(sockets, next_ephemeral_port)
            } else {
                Some(*port)
            };
            let ok = match (table.get(cap, owner), port) {
                (Some(h), Some(port)) => sockets.get_mut::<udp::Socket>(h).bind(port).is_ok(),
                _ => false,
            };
            if ok {
                table.set_state(cap, SocketState::Listening);
                send_typed(sender, R::Ok);
            } else {
                send_typed(sender, R::Err(NetError::Failure as u8));
            }
            true
        }
        NetRequest::UdpSend {
            cap_id,
            addr,
            port,
            data,
        } => {
            let cap = *cap_id as u64;
            let ep = IpEndpoint::new(IpAddress::v4(addr[0], addr[1], addr[2], addr[3]), *port);
            let n = match table.get(cap, owner) {
                Some(h)
                    if ensure_client_bound(sockets, h)
                        && sockets
                            .get_mut::<udp::Socket>(h)
                            .send_slice(data, ep)
                            .is_ok() =>
                {
                    iface.poll(now_instant(), device, sockets);
                    data.len()
                }
                _ => 0,
            };
            send_typed(sender, R::Data(&(n as u32).to_le_bytes()));
            true
        }
        NetRequest::UdpRecv { cap_id, buf_len } => {
            let cap = *cap_id as u64;
            let buf_len = (*buf_len as usize).min(512);
            let result = if let Some(h) = table.get(cap, owner) {
                let s = sockets.get_mut::<udp::Socket>(h);
                if s.can_recv() {
                    let mut raw = alloc::vec![0u8; buf_len];
                    s.recv_slice(&mut raw).ok().map(|(n, meta)| {
                        let IpAddress::Ipv4(src_ip) = meta.endpoint.addr;
                        let mut reply = alloc::vec![0u8; 6 + n];
                        reply[0..4].copy_from_slice(src_ip.as_bytes());
                        reply[4..6].copy_from_slice(&meta.endpoint.port.to_le_bytes());
                        reply[6..6 + n].copy_from_slice(&raw[..n]);
                        reply
                    })
                } else {
                    None
                }
            } else {
                None
            };
            match result {
                Some(reply) => {
                    let mut rb = [0u8; IPC_BUF_SIZE];
                    if let Ok(s) = ipc::encode(&R::Data(&reply), &mut rb) {
                        let _ = reply_to(sender, s);
                    }
                }
                None => send_typed(sender, R::Data(&[])),
            }
            true
        }
        _ => false,
    }
}
