//! Typed NetRequest dispatch for the net service cell.

extern crate alloc;

mod tcp;
#[cfg(test)]
mod tests;
mod udp;

use alloc::collections::BTreeMap;
use api::ipc::{self, NetRequest, NetResponse, IPC_BUF_SIZE};
use ostd::syscall::sys_send;
use smoltcp::{
    iface::{Interface, SocketHandle, SocketSet},
    socket::tcp as smoltcp_tcp,
    wire::IpAddress,
};

use crate::{
    interface::VirtioNetDevice,
    now_instant,
    socket_state::SocketState,
    socket_table::{SocketOwner, SocketTable},
    tls::socket::TlsSocketEntry,
    tls_handler::handle_tls_raw,
    tls_wire::TLS_CLOSE_OP,
};

pub(crate) fn tcp_state_byte(s: smoltcp_tcp::State) -> u8 {
    match s {
        smoltcp_tcp::State::Closed => 0x00,
        smoltcp_tcp::State::SynSent => 0x01,
        smoltcp_tcp::State::SynReceived => 0x02,
        smoltcp_tcp::State::Established => 0x03,
        smoltcp_tcp::State::FinWait1 => 0x04,
        smoltcp_tcp::State::FinWait2 => 0x05,
        smoltcp_tcp::State::CloseWait => 0x06,
        smoltcp_tcp::State::Closing => 0x07,
        smoltcp_tcp::State::LastAck => 0x08,
        smoltcp_tcp::State::TimeWait => 0x09,
        smoltcp_tcp::State::Listen => 0x0A,
    }
}

pub(crate) fn send_typed(sender: usize, resp: NetResponse<'_>) {
    let mut r = [0u8; IPC_BUF_SIZE];
    if let Ok(s) = ipc::encode(&resp, &mut r) {
        sys_send(sender, s);
    }
}

pub(crate) fn try_promote(
    table: &mut SocketTable,
    sockets: &mut SocketSet<'_>,
    cap: u64,
    owner: SocketOwner,
) {
    if table.get_state(cap, owner) == Some(SocketState::Connecting) {
        if let Some(h) = table.get(cap, owner) {
            if sockets.get_mut::<smoltcp_tcp::Socket>(h).state() == smoltcp_tcp::State::Established
            {
                table.set_state(cap, SocketState::Connected);
            }
        }
    }
}

pub(crate) fn make_tcp(
    sockets: &mut SocketSet<'_>,
    table: &mut SocketTable,
    owner: SocketOwner,
) -> Result<(SocketHandle, u64), ()> {
    let handle = sockets.add(smoltcp_tcp::Socket::new(
        smoltcp_tcp::SocketBuffer::new(alloc::vec![0u8; 4096]),
        smoltcp_tcp::SocketBuffer::new(alloc::vec![0u8; 4096]),
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
pub fn handle_request(
    buf: &[u8],
    sender: usize,
    owner: SocketOwner,
    iface: &mut Interface,
    device: &mut VirtioNetDevice,
    sockets: &mut SocketSet<'_>,
    table: &mut SocketTable,
    tls_table: &mut BTreeMap<u64, TlsSocketEntry>,
    local_ip: &[u8; 4],
) {
    match ipc::decode::<NetRequest<'_>>(buf) {
        Ok(req) => {
            iface.poll(now_instant(), device, sockets);
            handle_typed(
                req, sender, owner, iface, device, sockets, table, tls_table, local_ip,
            );
            iface.poll(now_instant(), device, sockets);
        }
        Err(_) if buf.first().copied().unwrap_or(0) >= TLS_CLOSE_OP => {
            iface.poll(now_instant(), device, sockets);
            handle_tls_raw(buf, sender, owner, iface, device, sockets, table, tls_table);
            iface.poll(now_instant(), device, sockets);
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_typed(
    req: NetRequest<'_>,
    sender: usize,
    owner: SocketOwner,
    iface: &mut Interface,
    device: &mut VirtioNetDevice,
    sockets: &mut SocketSet<'_>,
    table: &mut SocketTable,
    tls_table: &mut BTreeMap<u64, TlsSocketEntry>,
    local_ip: &[u8; 4],
) {
    use NetResponse as R;
    if tcp::handle_tcp_request(&req, sender, owner, iface, sockets, table, tls_table) {
        return;
    }
    if udp::handle_udp_request(&req, sender, owner, iface, device, sockets, table) {
        return;
    }

    match req {
        NetRequest::GetLocalIp => send_typed(sender, R::Addr(*local_ip)),
        NetRequest::MulticastJoin { cap_id: _, group } => {
            let g = IpAddress::v4(group[0], group[1], group[2], group[3]);
            let ok = iface.join_multicast_group(device, g, now_instant()).is_ok();
            send_typed(sender, if ok { R::Ok } else { R::Err(0xFF) });
        }
        NetRequest::MulticastLeave { cap_id: _, group } => {
            let g = IpAddress::v4(group[0], group[1], group[2], group[3]);
            let ok = iface
                .leave_multicast_group(device, g, now_instant())
                .is_ok();
            send_typed(sender, if ok { R::Ok } else { R::Err(0xFF) });
        }
        NetRequest::Resolve { .. } => send_typed(sender, R::Err(0xFF)),
        NetRequest::L2Send { data } => {
            let response = if device.send_l2(data) {
                R::Ok
            } else {
                R::Err(0xFF)
            };
            send_typed(sender, response);
        }
        NetRequest::L2Recv { guest_mac } => {
            device.set_guest_mac(guest_mac);
            let _ = device.pump_rx_split();
            if let Some(frame) = device.pop_guest_rx() {
                let mut rb = [0u8; IPC_BUF_SIZE];
                if let Ok(s) = ipc::encode(&R::Data(&frame), &mut rb) {
                    sys_send(sender, s);
                }
            } else {
                send_typed(sender, R::Ok);
            }
        }
        _ => {}
    }
}
