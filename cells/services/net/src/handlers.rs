//! Typed NetRequest dispatch for the net service cell.

extern crate alloc;

pub(crate) mod tcp;
#[cfg(test)]
mod tests;
mod udp;

use alloc::collections::{BTreeMap, BTreeSet};
use api::ipc::{self, NetRequest, NetResponse, IPC_BUF_SIZE};
use ostd::ipc::{post, reply_to};
use ostd::sync::Mutex;
use smoltcp::{
    iface::{Interface, SocketHandle, SocketSet},
    socket::tcp as smoltcp_tcp,
    wire::IpAddress,
};

use crate::{
    dns::Resolver,
    interface::VirtioNetDevice,
    service_runtime::now_instant,
    socket_state::SocketState,
    socket_table::{SocketOwner, SocketTable},
    tls::socket::TlsSocketEntry,
    tls_handler::{handle_tls_raw, TlsPending},
    tls_wire::TLS_CLOSE_OP,
};

/// L2 bridge requests handled, by direction (`loop-trace` images).
///
/// `l2recv` is the hypervisor's guest-RX poll: it is the count that proves the
/// guest's receive path reached this service at all.
pub static L2_SEND_REQUESTS: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
static MULTICAST_MEMBERS: Mutex<BTreeMap<[u8; 4], BTreeSet<usize>>> = Mutex::new(BTreeMap::new());
pub static L2_RECV_REQUESTS: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

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
        let _ = reply_to(sender, s);
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
    if !table.can_insert(owner) {
        return Err(());
    }
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
    tls_pending: &mut TlsPending,
    resolver: &Resolver,
    local_ip: &[u8; 4],
) {
    // Prevent typed TCP access to a TLS-owned socket while its future holds
    // the transport. Raw TLS Close is the cancellation/teardown path.
    if let Ok(req) = ipc::decode::<NetRequest<'_>>(buf) {
        let cap = match req {
            NetRequest::TcpSend { cap_id, .. }
            | NetRequest::TcpRecv { cap_id, .. }
            | NetRequest::TcpClose { cap_id }
            | NetRequest::TcpRecvReady { cap_id, .. }
            | NetRequest::TcpSendReady { cap_id, .. }
            | NetRequest::TcpCloseGraceful { cap_id }
            | NetRequest::SocketState { cap_id } => Some(cap_id as u64),
            _ => None,
        };
        if let Some(cap) = cap {
            if table.is_owner(cap, owner) && tls_pending.contains_cap(cap) {
                send_typed(sender, NetResponse::Err(0xFF));
                return;
            }
            if table.is_owner(cap, owner)
                && tls_table.contains_key(&cap)
                && !matches!(req, NetRequest::TcpClose { .. })
            {
                send_typed(sender, NetResponse::Err(0xFF));
                return;
            }
        }
    }
    match ipc::decode::<NetRequest<'_>>(buf) {
        Ok(req) => {
            iface.poll(now_instant(), device, sockets);
            handle_typed(
                req, sender, owner, iface, device, sockets, table, tls_table, resolver, local_ip,
            );
            iface.poll(now_instant(), device, sockets);
        }
        Err(_) if buf.first().copied().unwrap_or(0) >= TLS_CLOSE_OP => {
            iface.poll(now_instant(), device, sockets);
            handle_tls_raw(
                buf,
                sender,
                owner,
                iface,
                device,
                sockets,
                table,
                tls_table,
                tls_pending,
            );
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
    resolver: &Resolver,
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
            let mut members = MULTICAST_MEMBERS.lock();
            let set = members.entry(group).or_default();
            if set.contains(&sender) {
                send_typed(sender, R::Ok);
                return;
            }
            let is_first = set.is_empty();
            let g = IpAddress::v4(group[0], group[1], group[2], group[3]);
            let ok = if is_first {
                iface.join_multicast_group(device, g, now_instant()).is_ok()
            } else {
                true
            };
            if ok {
                set.insert(sender);
                send_typed(sender, R::Ok);
            } else {
                send_typed(sender, R::Err(0xFF));
            }
        }
        NetRequest::MulticastLeave { cap_id: _, group } => {
            let mut members = MULTICAST_MEMBERS.lock();
            let Some(set) = members.get_mut(&group) else {
                send_typed(sender, R::Err(0xFF));
                return;
            };
            if !set.remove(&sender) {
                send_typed(sender, R::Err(0xFF));
                return;
            }
            let is_last = set.is_empty();
            if is_last {
                members.remove(&group);
                let g = IpAddress::v4(group[0], group[1], group[2], group[3]);
                let _ = iface.leave_multicast_group(device, g, now_instant());
            }
            send_typed(sender, R::Ok);
        }
        NetRequest::Resolve { hostname } => {
            // The service reactor intercepts wire lookups and retains their
            // query slots. This fallback only handles static names.
            let answer = crate::dns::static_lookup(hostname, resolver.server());
            match answer {
                Some(addr) => send_typed(sender, R::Addr(addr)),
                None => send_typed(sender, R::Err(0xFF)),
            }
        }
        NetRequest::L2Send { data } => {
            L2_SEND_REQUESTS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if data.len() < 14 || data.len() > 1514 {
                send_typed(sender, R::Err(0xFF));
                return;
            }
            device.learn_guest_mac_from(data);
            if let Some(accepted) = device.send_l2(data, sender) {
                send_typed(sender, if accepted { R::Ok } else { R::Err(0xFF) });
            }
        }
        NetRequest::L2Recv { guest_mac } => {
            L2_RECV_REQUESTS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if guest_mac == [0; 6] || guest_mac == [0xFF; 6] || (guest_mac[0] & 1 != 0) {
                send_typed(sender, R::Err(0xFF));
                return;
            }
            let operation = ostd::ipc::current();
            static FIRST_GUEST_POLL: core::sync::atomic::AtomicBool =
                core::sync::atomic::AtomicBool::new(false);
            if !FIRST_GUEST_POLL.swap(true, core::sync::atomic::Ordering::Relaxed) {
                ostd::io::println("[net-bridge] first guest L2Recv — guest MAC registered");
            }
            device.set_guest_mac(guest_mac);
            let _ = device.pump_rx_split();
            if let Some(frame) = device.pop_guest_rx() {
                static FIRST_GUEST_RX: core::sync::atomic::AtomicBool =
                    core::sync::atomic::AtomicBool::new(false);
                if !FIRST_GUEST_RX.swap(true, core::sync::atomic::Ordering::Relaxed) {
                    ostd::io::println(&alloc::format!(
                        "[net-bridge] first guest RX len={}",
                        frame.len()
                    ));
                }
                let mut rb = [0u8; IPC_BUF_SIZE];
                if let Ok(s) = ipc::encode(&R::Data(&frame), &mut rb) {
                    if let Some(operation) = operation {
                        let _ = ostd::ipc::reply(operation, s);
                    } else {
                        let _ = post(sender, s);
                    }
                }
            } else {
                if let Some(operation) = operation {
                    let mut rb = [0u8; IPC_BUF_SIZE];
                    if let Ok(s) = ipc::encode(&R::Err(0), &mut rb) {
                        let _ = ostd::ipc::reply(operation, s);
                    }
                } else {
                    send_typed(sender, R::Err(0));
                }
            }
        }
        _ => {}
    }
}
