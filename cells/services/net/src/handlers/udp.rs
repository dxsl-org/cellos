use api::ipc::{self, NetRequest, NetResponse, IPC_BUF_SIZE};
use ostd::syscall::sys_send;
use smoltcp::{
    iface::{Interface, SocketSet},
    socket::udp,
    wire::{IpAddress, IpEndpoint},
};

use super::send_typed;
use crate::{
    interface::VirtioNetDevice,
    next_ephemeral_port, now_instant,
    socket_state::SocketState,
    socket_table::{SocketOwner, SocketTable},
};

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
                    send_typed(sender, R::Err(0xFF));
                }
            }
            true
        }
        NetRequest::UdpBind { cap_id, port } => {
            let cap = *cap_id as u64;
            let port = if *port == 0 {
                next_ephemeral_port()
            } else {
                *port
            };
            let ok = if let Some(h) = table.get(cap, owner) {
                sockets.get_mut::<udp::Socket>(h).bind(port).is_ok()
            } else {
                false
            };
            if ok {
                table.set_state(cap, SocketState::Listening);
                send_typed(sender, R::Ok);
            } else {
                send_typed(sender, R::Err(0xFF));
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
            let n = if let Some(h) = table.get(cap, owner) {
                if sockets
                    .get_mut::<udp::Socket>(h)
                    .send_slice(data, ep)
                    .is_ok()
                {
                    iface.poll(now_instant(), device, sockets);
                    data.len()
                } else {
                    0
                }
            } else {
                0
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
                        sys_send(sender, s);
                    }
                }
                None => send_typed(sender, R::Data(&[])),
            }
            true
        }
        _ => false,
    }
}
