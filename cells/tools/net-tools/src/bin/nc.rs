#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

use api::ipc::{NetRequest, NetResponse, IPC_BUF_SIZE};
use api::syscall::service;
use ostd::io::{print, println};
use ostd::syscall::{sys_lookup_service, sys_recv, sys_send, sys_yield, SyscallResult};

/// Payload sent and expected back from the echo server.
const HELLO: &[u8] = b"HELLO_ViCell\n";

api::declare_syscalls![Send, Recv, Log, StateRestore, LookupService];

ostd::cell_main!(cell_main);

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
    let net_ep = match sys_lookup_service(service::NET) {
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
        Some(a) => a,
        None => {
            println("nc: cannot resolve host");
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
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let len = api::ipc::encode(&NetRequest::TcpConnect { addr, port }, &mut req_buf)
        .map(|b| b.len())
        .unwrap_or(0);
    sys_send(net_ep, &req_buf[..len]);
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let cap_id = match sys_recv(0, &mut resp_buf) {
        SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&resp_buf) {
            Ok(NetResponse::CapId(c)) => c,
            _ => {
                println("nc: connect failed");
                return;
            }
        },
        _ => {
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
        let mut send_buf = [0u8; IPC_BUF_SIZE];
        let send_len = api::ipc::encode(&NetRequest::TcpSend { cap_id, data: rem }, &mut send_buf)
            .map(|b| b.len())
            .unwrap_or(0);
        sys_send(net_ep, &send_buf[..send_len]);
        let mut cnt_buf = [0u8; IPC_BUF_SIZE];
        match sys_recv(0, &mut cnt_buf) {
            SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&cnt_buf) {
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
            },
            _ => break,
        }
    }

    // ── Recv echo — poll until data arrives ───────────────────────────────────
    let mut recv_req_buf = [0u8; IPC_BUF_SIZE];
    let recv_req_len = api::ipc::encode(
        &NetRequest::TcpRecv {
            cap_id,
            buf_len: 256,
        },
        &mut recv_req_buf,
    )
    .map(|b| b.len())
    .unwrap_or(0);

    for _ in 0..500 {
        sys_send(net_ep, &recv_req_buf[..recv_req_len]);
        let mut data_buf = [0u8; IPC_BUF_SIZE];
        match sys_recv(0, &mut data_buf) {
            SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&data_buf) {
                Ok(NetResponse::Data(b)) if !b.is_empty() => {
                    if let Ok(s) = core::str::from_utf8(b) {
                        print(s);
                    }
                    break;
                }
                _ => {
                    sys_yield();
                }
            },
            _ => break,
        }
    }

    close_socket(cap_id, net_ep);
}

/// nc -l <port> — listen, accept one connection, echo bytes to serial and
/// back to the peer, then close when the peer closes.
fn server_mode(port: u16, net_ep: usize) {
    // TcpListen (atomic create + listen)
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let len = api::ipc::encode(&NetRequest::TcpListen { port }, &mut req_buf)
        .map(|b| b.len())
        .unwrap_or(0);
    sys_send(net_ep, &req_buf[..len]);
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let listen_cap = match sys_recv(0, &mut resp_buf) {
        SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&resp_buf) {
            Ok(NetResponse::CapId(c)) => c,
            _ => {
                println("nc: listen failed");
                return;
            }
        },
        _ => {
            println("nc: TcpListen syscall failed");
            return;
        }
    };
    print("listening on ");
    ostd::io::print_usize(port as usize);
    println("");

    // Pre-encode TcpAccept for the listen cap (reused across accept polls).
    let mut accept_req_buf = [0u8; IPC_BUF_SIZE];
    let accept_req_len = api::ipc::encode(
        &NetRequest::TcpAccept { cap_id: listen_cap },
        &mut accept_req_buf,
    )
    .map(|b| b.len())
    .unwrap_or(0);

    let stream_cap: u32 = loop {
        sys_send(net_ep, &accept_req_buf[..accept_req_len]);
        let mut r = [0u8; IPC_BUF_SIZE];
        match sys_recv(0, &mut r) {
            SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&r) {
                Ok(NetResponse::CapId(c)) => break c,
                _ => {
                    sys_yield();
                }
            },
            _ => {
                sys_yield();
            }
        }
    };
    println("connected");

    serve_connection(stream_cap, net_ep);

    // Accept loop — keep accepting connections on the same listener.
    loop {
        println("waiting for next connection");
        let next_cap: u32 = loop {
            sys_send(net_ep, &accept_req_buf[..accept_req_len]);
            let mut r = [0u8; IPC_BUF_SIZE];
            match sys_recv(0, &mut r) {
                SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&r) {
                    Ok(NetResponse::CapId(c)) => break c,
                    _ => {
                        sys_yield();
                    }
                },
                _ => {
                    sys_yield();
                }
            }
        };
        println("connected");
        serve_connection(next_cap, net_ep);
    }
}

/// Recv loop: print received bytes to serial and echo back to peer.
/// Exits when the peer closes the connection.
fn serve_connection(cap: u32, net_ep: usize) {
    let mut recv_req_buf = [0u8; IPC_BUF_SIZE];
    let recv_req_len = api::ipc::encode(
        &NetRequest::TcpRecv {
            cap_id: cap,
            buf_len: 256,
        },
        &mut recv_req_buf,
    )
    .map(|b| b.len())
    .unwrap_or(0);

    'recv: for _ in 0..500_000 {
        sys_send(net_ep, &recv_req_buf[..recv_req_len]);
        let mut data_buf = [0u8; IPC_BUF_SIZE];
        match sys_recv(0, &mut data_buf) {
            SyscallResult::Ok(_) => {
                match api::ipc::decode::<NetResponse>(&data_buf) {
                    Ok(NetResponse::Data(b)) if !b.is_empty() => {
                        if let Ok(s) = core::str::from_utf8(b) {
                            print(s);
                        }
                        // Echo back to peer.
                        let mut echo_buf = [0u8; IPC_BUF_SIZE];
                        let echo_len = api::ipc::encode(
                            &NetRequest::TcpSend {
                                cap_id: cap,
                                data: b,
                            },
                            &mut echo_buf,
                        )
                        .map(|e| e.len())
                        .unwrap_or(0);
                        sys_send(net_ep, &echo_buf[..echo_len]);
                        let mut cnt_buf = [0u8; IPC_BUF_SIZE];
                        let _ = sys_recv(0, &mut cnt_buf);
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
            _ => break,
        }
    }
    close_socket(cap, net_ep);
}

fn query_state(cap_id: u32, net_ep: usize) -> u8 {
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let len = api::ipc::encode(&NetRequest::SocketState { cap_id }, &mut req_buf)
        .map(|b| b.len())
        .unwrap_or(0);
    sys_send(net_ep, &req_buf[..len]);
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match sys_recv(0, &mut resp_buf) {
        SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&resp_buf) {
            Ok(NetResponse::State(s)) => s,
            _ => 0x00,
        },
        _ => 0x00,
    }
}

fn close_socket(cap_id: u32, net_ep: usize) {
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let len = api::ipc::encode(&NetRequest::TcpClose { cap_id }, &mut req_buf)
        .map(|b| b.len())
        .unwrap_or(0);
    sys_send(net_ep, &req_buf[..len]);
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let _ = sys_recv(0, &mut resp_buf);
}

/// Resolve `host` through the net service (`NetRequest::Resolve`).
///
/// The service owns the whole resolution order — IPv4 literal, SLIRP alias
/// (`gateway`, `host`, `dns`, `localhost`), then a UDP A-record query — so the
/// tools never carry a second, drifting copy of it.
fn resolve_host(host: &str, net_ep: usize) -> Option<[u8; 4]> {
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let len = api::ipc::encode(&NetRequest::Resolve { hostname: host }, &mut req_buf)
        .ok()?
        .len();
    sys_send(net_ep, &req_buf[..len]);
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match sys_recv(0, &mut resp_buf) {
        SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&resp_buf) {
            Ok(NetResponse::Addr(addr)) => Some(addr),
            _ => None,
        },
        _ => None,
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
