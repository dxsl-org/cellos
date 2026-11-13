#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![forbid(unsafe_code)]
extern crate ostd;

use abi::syscall::service;
use ostd::io::{print, println};
use ostd::ipc::{service_call_typed_bounded, IpcError};
use ostd::service::provider_tid;
use ostd::syscall::sys_yield;
use wire::ipc::{NetRequest, NetResponse, IPC_BUF_SIZE};
use wire::proto::net::NetError;

/// service-net refused the request: this Cell holds no network right for it.
const NET_DENIED: &str = "permission denied (no network right)";

/// Payload sent and expected back from the echo server.
const HELLO: &[u8] = b"HELLO_ViCell\n";

/// 10 s: one net-service request; covers a `Resolve` the service holds until
/// its own DNS query deadline.
const NET_CALL_TICKS: u64 = 1_000;

abi::declare_syscalls![Send, Recv, Log, StateRestore, LookupServiceBound, GetTime];

ostd::cell_main!(cell_main);

/// Exchange one `NetRequest` with service-net as an exact operation.
///
/// The service answers every request with exactly one reply. `Err` is a dead
/// or unanswering service (`PeerGone`, `PreDispatchTimeout`, `Indeterminate`),
/// an unencodable request, or an undecodable reply (`Decode`).
fn net_call<'r>(
    net_ep: usize,
    req: &NetRequest<'_>,
    reply: &'r mut [u8; IPC_BUF_SIZE],
) -> Result<NetResponse<'r>, IpcError> {
    let mut send = [0u8; IPC_BUF_SIZE];
    service_call_typed_bounded(net_ep, req, &mut send, reply, NET_CALL_TICKS)
}

/// nc <host> <port>  |  nc -l <port>
fn cell_main() {
    // ── Parse argv ───────────────────────────────────────────────────────────
    let argv = ostd::args();
    if argv.is_empty() {
        println("Usage: nc <host> <port>  |  nc -l <port>");
        return;
    }
    let mut parts = argv.iter().map(|arg| arg.as_str());
    let first = match parts.next() {
        Some(t) => t,
        None => {
            println("Usage: nc <host> <port>  |  nc -l <port>");
            return;
        }
    };

    // ── Resolve net service endpoint ──────────────────────────────────────────
    let net_ep = match provider_tid(service::NET) {
        Some(ep) => ep,
        None => {
            println("nc: no net service");
            return;
        }
    };

    if first == "-l" {
        let port = match parts.next().and_then(parse_u16) {
            Some(p) => p,
            None => {
                println("Usage: nc -l <port>");
                return;
            }
        };
        server_mode(port, net_ep);
        return;
    }

    let host = first;
    let port_str = match parts.next() {
        Some(p) => p,
        None => {
            println("Usage: nc <host> <port>");
            return;
        }
    };
    let addr = match resolve_host(host, net_ep) {
        Ok(a) => a,
        Err(why) => {
            print("nc: ");
            println(why);
            return;
        }
    };
    let port: u16 = match parse_u16(port_str) {
        Some(p) => p,
        None => {
            println("nc: invalid port");
            return;
        }
    };

    // ── TcpConnect (atomic create + connect) ─────────────────────────────────
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let cap_id = match net_call(
        net_ep,
        &NetRequest::TcpConnect { addr, port },
        &mut resp_buf,
    ) {
        Ok(NetResponse::CapId(c)) => c,
        Ok(NetResponse::Err(code)) if code == NetError::Denied as u8 => {
            print("nc: connect: ");
            println(NET_DENIED);
            return;
        }
        Ok(_) | Err(IpcError::Decode) => {
            println("nc: connect failed");
            return;
        }
        Err(_) => {
            println("nc: TcpConnect syscall failed");
            return;
        }
    };
    println("connected");

    // ── Send "HELLO_ViCell\n" via TcpSend with retry ──────────────────────────
    let mut sent_bytes = 0usize;
    for _ in 0..500 {
        if sent_bytes >= HELLO.len() {
            break;
        }
        let rem = &HELLO[sent_bytes..];
        let mut cnt_buf = [0u8; IPC_BUF_SIZE];
        match net_call(
            net_ep,
            &NetRequest::TcpSend { cap_id, data: rem },
            &mut cnt_buf,
        ) {
            Ok(NetResponse::Data(b)) if b.len() >= 4 => {
                let mut arr = [0u8; 4];
                arr.copy_from_slice(&b[0..4]);
                let n = u32::from_le_bytes(arr) as usize;
                sent_bytes += n;
                if n == 0 {
                    sys_yield();
                }
            }
            _ => break,
        }
    }

    // ── Recv echo — poll until data arrives ───────────────────────────────────
    let recv_req = NetRequest::TcpRecv {
        cap_id,
        buf_len: 256,
    };
    for _ in 0..500 {
        let mut data_buf = [0u8; IPC_BUF_SIZE];
        match net_call(net_ep, &recv_req, &mut data_buf) {
            Ok(NetResponse::Data(b)) if !b.is_empty() => {
                if let Ok(s) = core::str::from_utf8(b) {
                    print(s);
                }
                break;
            }
            Ok(_) | Err(IpcError::Decode) => {
                sys_yield();
            }
            Err(_) => break,
        }
    }

    close_socket(cap_id, net_ep);
}

/// nc -l <port> — listen, accept one connection, echo bytes to serial and
/// back to the peer, then close when the peer closes.
fn server_mode(port: u16, net_ep: usize) {
    // TcpListen (atomic create + listen)
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let listen_cap = match net_call(net_ep, &NetRequest::TcpListen { port }, &mut resp_buf) {
        Ok(NetResponse::CapId(c)) => c,
        Ok(NetResponse::Err(code)) if code == NetError::Denied as u8 => {
            print("nc: listen: ");
            println(NET_DENIED);
            return;
        }
        Ok(_) | Err(IpcError::Decode) => {
            println("nc: listen failed");
            return;
        }
        Err(_) => {
            println("nc: TcpListen syscall failed");
            return;
        }
    };
    print("listening on ");
    ostd::io::print_usize(port as usize);
    println("");

    let Some(stream_cap) = accept_next(listen_cap, net_ep) else {
        println("nc: accept failed");
        return;
    };
    println("connected");

    serve_connection(stream_cap, net_ep);

    // Accept loop — keep accepting connections on the same listener.
    loop {
        println("waiting for next connection");
        let Some(next_cap) = accept_next(listen_cap, net_ep) else {
            println("nc: accept failed");
            return;
        };
        println("connected");
        serve_connection(next_cap, net_ep);
    }
}

/// Poll `TcpAccept` on `listen_cap` until a connection arrives.
///
/// "Nothing pending" and a slow service are retried; `None` only when the net
/// service is gone, since no later poll can then succeed.
fn accept_next(listen_cap: u32, net_ep: usize) -> Option<u32> {
    let accept_req = NetRequest::TcpAccept { cap_id: listen_cap };
    loop {
        let mut r = [0u8; IPC_BUF_SIZE];
        match net_call(net_ep, &accept_req, &mut r) {
            Ok(NetResponse::CapId(c)) => return Some(c),
            Err(IpcError::PeerGone) => return None,
            _ => sys_yield(),
        }
    }
}

/// Recv loop: print received bytes to serial and echo back to peer.
/// Exits when the peer closes the connection.
fn serve_connection(cap: u32, net_ep: usize) {
    let recv_req = NetRequest::TcpRecv {
        cap_id: cap,
        buf_len: 256,
    };

    'recv: for _ in 0..500_000 {
        let mut data_buf = [0u8; IPC_BUF_SIZE];
        match net_call(net_ep, &recv_req, &mut data_buf) {
            Ok(NetResponse::Data(b)) if !b.is_empty() => {
                if let Ok(s) = core::str::from_utf8(b) {
                    print(s);
                }
                // Echo back to peer.
                let mut cnt_buf = [0u8; IPC_BUF_SIZE];
                let _ = net_call(
                    net_ep,
                    &NetRequest::TcpSend {
                        cap_id: cap,
                        data: b,
                    },
                    &mut cnt_buf,
                );
            }
            Ok(NetResponse::Data(_)) => {
                let st = query_state(cap, net_ep);
                if st == 0x06 || st == 0x00 {
                    break 'recv;
                }
                sys_yield();
            }
            _ => break,
        }
    }
    close_socket(cap, net_ep);
}

fn query_state(cap_id: u32, net_ep: usize) -> u8 {
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match net_call(net_ep, &NetRequest::SocketState { cap_id }, &mut resp_buf) {
        Ok(NetResponse::State(s)) => s,
        _ => 0x00,
    }
}

fn close_socket(cap_id: u32, net_ep: usize) {
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let _ = net_call(net_ep, &NetRequest::TcpClose { cap_id }, &mut resp_buf);
}

/// Resolve `host` through the net service (`NetRequest::Resolve`).
///
/// The service owns the whole resolution order — IPv4 literal, SLIRP alias
/// (`gateway`, `host`, `dns`, `localhost`), then a UDP A-record query — so the
/// tools never carry a second, drifting copy of it.
/// The error is the reason to print: a denied lookup is not a DNS failure.
fn resolve_host(host: &str, net_ep: usize) -> Result<[u8; 4], &'static str> {
    const UNRESOLVED: &str = "cannot resolve host";
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match net_call(
        net_ep,
        &NetRequest::Resolve { hostname: host },
        &mut resp_buf,
    ) {
        Ok(NetResponse::Addr(addr)) => Ok(addr),
        Ok(NetResponse::Err(code)) if code == NetError::Denied as u8 => Err(NET_DENIED),
        _ => Err(UNRESOLVED),
    }
}

fn parse_u16(s: &str) -> Option<u16> {
    let mut n: u32 = 0;
    if s.is_empty() {
        return None;
    }
    for ch in s.bytes() {
        if !ch.is_ascii_digit() {
            return None;
        }
        n = n * 10 + (ch - b'0') as u32;
        if n > 65535 {
            return None;
        }
    }
    Some(n as u16)
}
