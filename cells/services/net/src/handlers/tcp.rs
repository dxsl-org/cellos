use alloc::collections::BTreeMap;
use api::ipc::{net_ready, NetRequest, NetResponse, TcpInterest, TcpReadyEvent, NET_TCP_INLINE_DATA_MAX};
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
            let buf_len = (*buf_len as usize).min(NET_TCP_INLINE_DATA_MAX);
            if let Some(h) = table.get(cap, owner) {
                let s = sockets.get_mut::<tcp::Socket>(h);
                if s.can_recv() {
                    let _ = s.recv(|data| {
                        let n = data.len().min(buf_len);
                        send_typed(sender, R::Data(&data[..n]));
                        (n, ())
                    });
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
            // Prepare a replacement listener first. If its listen fails or
            // capacity is exhausted, the established handle stays attached to
            // the old cap and the caller can retry without a leaked stream cap.
            if !table.can_insert(owner) {
                send_typed(sender, R::Err(0xFF));
                return true;
            }
            let mut replacement = tcp::Socket::new(
                tcp::SocketBuffer::new(alloc::vec![0u8; 4096]),
                tcp::SocketBuffer::new(alloc::vec![0u8; 4096]),
            );
            if replacement.listen(listen_port).is_err() {
                send_typed(sender, R::Err(0xFF));
                return true;
            }
            let nh = sockets.add(replacement);
            match table.insert_with_state(handle, SocketState::Connected, owner) {
                Ok(stream_cap) => {
                    table.update_handle(cap, nh);
                    send_typed(sender, R::CapId(stream_cap as u32));
                }
                Err(_) => {
                    sockets.remove(nh);
                    send_typed(sender, R::Err(0xFF));
                }
            }
            true
        }
        NetRequest::TcpRecvReady { cap_id, buf_len } => {
            let cap = *cap_id as u64;
            if table.is_udp(cap, owner) || *buf_len == 0 {
                send_typed(sender, R::Err(0xFF));
            } else if let Some(h) = table.get(cap, owner) {
                let s = sockets.get_mut::<tcp::Socket>(h);
                if s.can_recv() {
                    let limit = (*buf_len as usize).min(NET_TCP_INLINE_DATA_MAX);
                    let _ = s.recv(|data| {
                        let n = data.len().min(limit);
                        send_typed(sender, if n > 0 { R::Data(&data[..n]) } else { R::NotReady });
                        (n, ())
                    });
                } else if !s.may_recv() {
                    send_typed(sender, R::Eof);
                } else {
                    send_typed(sender, R::NotReady);
                }
            } else {
                send_typed(sender, R::Err(0xFF));
            }
            true
        }
        NetRequest::TcpSendReady { cap_id, data } => {
            let cap = *cap_id as u64;
            if table.is_udp(cap, owner) || table.is_graceful_closing(cap) {
                send_typed(sender, R::Err(0xFF));
            } else if let Some(h) = table.get(cap, owner) {
                let s = sockets.get_mut::<tcp::Socket>(h);
                if !s.may_send() {
                    send_typed(sender, R::Eof);
                } else if !s.can_send() {
                    send_typed(sender, R::NotReady);
                } else {
                    let n = s.send_slice(data).unwrap_or(0);
                    send_typed(sender, if n == 0 && !data.is_empty() {
                        R::NotReady
                    } else {
                        R::WriteProgress(n as u32)
                    });
                }
            } else {
                send_typed(sender, R::Err(0xFF));
            }
            true
        }
        NetRequest::TcpCloseGraceful { cap_id } => {
            let cap = *cap_id as u64;
            if table.is_udp(cap, owner) {
                send_typed(sender, R::Err(0xFF));
            } else if let Some(h) = table.get(cap, owner) {
                if !table.is_graceful_closing(cap) {
                    sockets.get_mut::<tcp::Socket>(h).close();
                    table.begin_graceful_close(
                        cap,
                        ostd::syscall::sys_get_time().saturating_add(100_000_000),
                    );
                }
                send_typed(sender, R::Ok);
            } else {
                send_typed(sender, R::Err(0xFF));
            }
            true
        }
        NetRequest::TcpReady { interests, cursor, wait } => {
            if *wait {
                // The runtime owns deferred waits; a direct dispatch cannot
                // retain the receive token and must refuse rather than poll.
                send_typed(sender, R::Err(0xFD));
            } else {
                match readiness(interests, *cursor, owner, sockets, table) {
                    Ok((events, next_cursor)) => {
                        let mut packed = [0u8; READY_PAGE * api::ipc::NET_READY_RECORD_BYTES];
                        let bytes = pack_events(&events, &mut packed);
                        send_typed(sender, R::TcpReady { events: bytes, next_cursor });
                    }
                    Err(()) => send_typed(sender, R::Err(0xFF)),
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

pub(crate) const READY_PAGE: usize = 32;
pub(crate) const MAX_INTERESTS: usize = 256;
pub(crate) type ReadyEvents = heapless::Vec<TcpReadyEvent, READY_PAGE>;

/// Level-triggered readiness after interface progress. Every cap is validated
/// before returning any events; foreign owners cannot probe another owner's
/// socket even if its cap is mixed into an otherwise valid batch.
pub(crate) fn readiness(
    interests: &[u8],
    cursor: u16,
    owner: SocketOwner,
    sockets: &mut SocketSet<'_>,
    table: &SocketTable,
) -> Result<(ReadyEvents, u16), ()> {
    if interests.is_empty() || interests.len() > MAX_INTERESTS * api::ipc::NET_READY_RECORD_BYTES
        || interests.len() % api::ipc::NET_READY_RECORD_BYTES != 0
    {
        return Err(());
    }
    let count = interests.len() / api::ipc::NET_READY_RECORD_BYTES;
    for raw in interests.chunks_exact(api::ipc::NET_READY_RECORD_BYTES) {
        let interest = TcpInterest::decode(raw).ok_or(())?;
        if interest.mask == 0
            || interest.mask & !(net_ready::ACCEPT | net_ready::READ | net_ready::WRITE | net_ready::EOF | net_ready::ERROR) != 0
            || table.is_udp(interest.cap_id as u64, owner)
            || table.get(interest.cap_id as u64, owner).is_none()
        {
            return Err(());
        }
    }
    let mut events = ReadyEvents::new();
    let start = cursor as usize % count;
    let mut next = start;
    for _ in 0..count {
        let interest = TcpInterest::decode(&interests[next * api::ipc::NET_READY_RECORD_BYTES..(next + 1) * api::ipc::NET_READY_RECORD_BYTES]).ok_or(())?;
        let cap = interest.cap_id as u64;
        let handle = table.get(cap, owner).ok_or(())?;
        let socket = sockets.get_mut::<tcp::Socket>(handle);
        let mut ready = 0;
        if table.get_listen_port(cap, owner).is_some() {
            if socket.state() == tcp::State::Established && interest.mask & net_ready::ACCEPT != 0 {
                ready |= net_ready::ACCEPT;
            }
            if socket.state() == tcp::State::Closed {
                ready |= net_ready::ERROR;
            }
        } else {
            if socket.can_recv() && interest.mask & net_ready::READ != 0 {
                ready |= net_ready::READ;
            }
            if socket.can_send()
                && !table.is_graceful_closing(cap)
                && interest.mask & net_ready::WRITE != 0
            {
                ready |= net_ready::WRITE;
            }
            // CLOSE-WAIT (peer shutdown of its write half) can still serve an
            // HTTP response. EOF-only monitors indicate unusable TX, not FIN.
            if !socket.may_send() && !socket.is_active() {
                ready |= net_ready::EOF;
            }
            if socket.state() == tcp::State::Closed && !table.is_graceful_closing(cap) {
                ready |= net_ready::ERROR;
            }
        }
        if ready != 0 {
            events.push(TcpReadyEvent { cap_id: interest.cap_id, ready }).map_err(|_| ())?;
        }
        next = (next + 1) % count;
        if events.len() == READY_PAGE {
            break;
        }
    }
    Ok((events, next as u16))
}

pub(crate) fn pack_events<'a>(
    events: &[TcpReadyEvent],
    out: &'a mut [u8; READY_PAGE * api::ipc::NET_READY_RECORD_BYTES],
) -> &'a [u8] {
    let len = events.len() * api::ipc::NET_READY_RECORD_BYTES;
    for (event, chunk) in events.iter().zip(out[..len].chunks_exact_mut(api::ipc::NET_READY_RECORD_BYTES)) {
        chunk[..4].copy_from_slice(&event.cap_id.to_le_bytes());
        chunk[4] = event.ready;
    }
    &out[..len]
}
