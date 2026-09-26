//! wget — HTTP/1.0 file downloader for ViCell.
//!
//! Usage: wget http://HOST[:PORT][/path] <vfs_path>
//!
//! Downloads the URL body and writes it to `<vfs_path>` via typed VFS Write IPC.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

use api::ipc::{NetRequest, NetResponse, IPC_BUF_SIZE};
use api::syscall::service;
use ostd::io::println;
use ostd::syscall::{
    sys_get_time_ms, sys_lookup_service, sys_recv, sys_send, sys_yield, SyscallResult,
};

const RESP_BUF: usize = 4096;

/// Wall-clock ceiling for the send + receive phases of one download.
///
/// The loopback mocks answer within microseconds, but a public-internet round
/// trip (ARP, SYN, handshake, server think time) is orders of magnitude slower
/// while every poll costs one IPC round-trip with the net service. A fixed
/// iteration count therefore races the network; this budget does not.
const EXCHANGE_BUDGET_MS: u64 = 30_000;
/// Hard stop when the wall clock is unavailable or never advances.
const EXCHANGE_POLL_CEILING: usize = 200_000;

api::declare_syscalls![
    Send,
    Recv,
    Log,
    StateRestore,
    LookupService,
    VfsMutate,
    GetTime
];

ostd::cell_main!(cell_main);

fn cell_main() {
    let argv = ostd::args();
    if argv.is_empty() {
        println("Usage: wget http://HOST[:PORT][/path] <vfs_path>");
        return;
    }
    let url = match argv.first() {
        Some(u) => u,
        None => {
            println("wget: missing URL");
            return;
        }
    };
    let vfs_path = match argv.get(1) {
        Some(p) => p.as_str(),
        None => {
            println("wget: missing output path");
            return;
        }
    };

    let (host, port, path) = match parse_url(url) {
        Some(t) => t,
        None => {
            println("wget: invalid URL — expected http://HOST[:PORT][/path]");
            return;
        }
    };

    // ── Resolve service endpoints ─────────────────────────────────────────────
    let net_ep = match sys_lookup_service(service::NET) {
        Some(ep) => ep,
        None => {
            println("wget: no net service");
            return;
        }
    };
    let vfs_ep = match sys_lookup_service(service::VFS) {
        Some(ep) => ep,
        None => {
            println("wget: no vfs service");
            return;
        }
    };

    // ── Resolve the host through the service ─────────────────────────────────
    // IPv4 literals and the SLIRP aliases are answered service-side, DNS names
    // by its A-record resolver: the tools carry no resolver of their own.
    let addr = match resolve_host(host, net_ep) {
        Some(a) => a,
        None => {
            println("wget: cannot resolve host");
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
                println("wget: connect failed");
                return;
            }
        },
        _ => {
            println("wget: TcpConnect syscall failed");
            return;
        }
    };

    // ── Build and send HTTP GET request ───────────────────────────────────────
    let overhead =
        b"GET ".len() + b" HTTP/1.0\r\nHost: ".len() + b"\r\nConnection: close\r\n\r\n".len();
    if overhead + path.len() + host.len() > 500 {
        println("wget: URL too long");
        close_socket(cap_id, net_ep);
        return;
    }
    let mut request_data = [0u8; 512];
    let mut pos = 0usize;
    pos = wb(&mut request_data, pos, b"GET ");
    pos = wb(&mut request_data, pos, path.as_bytes());
    pos = wb(&mut request_data, pos, b" HTTP/1.0\r\nHost: ");
    pos = wb(&mut request_data, pos, host.as_bytes());
    pos = wb(&mut request_data, pos, b"\r\nConnection: close\r\n\r\n");
    let request_len = pos;

    let started_ms = sys_get_time_ms().unwrap_or(0);
    let mut sent_bytes = 0usize;
    for _ in 0..EXCHANGE_POLL_CEILING {
        if sent_bytes >= request_len || exchange_expired(started_ms) {
            break;
        }
        let rem = &request_data[sent_bytes..request_len];
        let chunk = rem.len().min(480);
        let mut send_buf = [0u8; IPC_BUF_SIZE];
        let send_len = api::ipc::encode(
            &NetRequest::TcpSend {
                cap_id,
                data: &rem[..chunk],
            },
            &mut send_buf,
        )
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

    // ── Accumulate HTTP response via TcpRecv ──────────────────────────────────
    let mut response = [0u8; RESP_BUF];
    let mut resp_len = 0usize;

    let mut recv_req_buf = [0u8; IPC_BUF_SIZE];
    let recv_req_len = api::ipc::encode(
        &NetRequest::TcpRecv {
            cap_id,
            buf_len: 500,
        },
        &mut recv_req_buf,
    )
    .map(|b| b.len())
    .unwrap_or(0);

    'recv: for _ in 0..EXCHANGE_POLL_CEILING {
        sys_send(net_ep, &recv_req_buf[..recv_req_len]);
        let mut data_buf = [0u8; IPC_BUF_SIZE];
        match sys_recv(0, &mut data_buf) {
            SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&data_buf) {
                Ok(NetResponse::Data(b)) if !b.is_empty() => {
                    let n = b.len().min(RESP_BUF - resp_len);
                    response[resp_len..resp_len + n].copy_from_slice(&b[..n]);
                    resp_len += n;
                    if resp_len >= RESP_BUF {
                        break 'recv;
                    }
                }
                Ok(NetResponse::Data(_)) => {
                    let st = query_state(cap_id, net_ep);
                    if st == 0x06 || st == 0x00 {
                        sys_send(net_ep, &recv_req_buf[..recv_req_len]);
                        let mut fb = [0u8; IPC_BUF_SIZE];
                        if let SyscallResult::Ok(_) = sys_recv(0, &mut fb) {
                            if let Ok(NetResponse::Data(b)) = api::ipc::decode::<NetResponse>(&fb) {
                                let n = b.len().min(RESP_BUF - resp_len);
                                response[resp_len..resp_len + n].copy_from_slice(&b[..n]);
                                resp_len += n;
                            }
                        }
                        break 'recv;
                    }
                    if exchange_expired(started_ms) {
                        break 'recv;
                    }
                    sys_yield();
                }
                _ => break,
            },
            _ => break,
        }
    }
    close_socket(cap_id, net_ep);

    // ── Extract body and write to VFS ─────────────────────────────────────────
    let resp = &response[..resp_len];
    let body = resp
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| &resp[i + 4..])
        .unwrap_or(resp);

    if body.is_empty() {
        println("wget: empty response body");
        return;
    }

    // Typed postcard IPC — VFS dropped the raw OP_* byte protocol in Phase 27;
    // the old OP_WRITE frame decoded as garbage and every write failed.
    // Content cap mirrors the 512-byte IPC frame minus path + envelope.
    let cl = body.len().min(440usize.saturating_sub(vfs_path.len()));
    let mut vfs_req = [0u8; 512];
    let req = api::ipc::VfsRequest::Write {
        path: vfs_path,
        content: &body[..cl],
    };
    let n = match api::ipc::encode(&req, &mut vfs_req) {
        Ok(s) => s.len(),
        Err(_) => {
            println("wget: request too large");
            return;
        }
    };
    sys_send(vfs_ep, &vfs_req[..n]);
    let mut r = [0u8; 64];
    match sys_recv(0, &mut r) {
        SyscallResult::Ok(_) => match api::ipc::decode::<api::ipc::VfsResponse>(&r) {
            Ok(api::ipc::VfsResponse::Ok) => {
                ostd::io::print("wget: saved ");
                ostd::io::print_usize(cl);
                ostd::io::print(" bytes to ");
                println(vfs_path);
            }
            _ => println("wget: VFS write failed"),
        },
        _ => println("wget: VFS write failed"),
    }
}

/// Has the download exceeded its wall-clock budget?
///
/// A missing clock (`GetTime` unavailable) reports "not expired" so the
/// iteration ceiling stays the only bound rather than cutting the download off
/// at the first poll.
fn exchange_expired(started_ms: u64) -> bool {
    sys_get_time_ms().is_some_and(|now| now.saturating_sub(started_ms) > EXCHANGE_BUDGET_MS)
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
            _ => 0,
        },
        _ => 0,
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

fn wb(buf: &mut [u8], pos: usize, src: &[u8]) -> usize {
    buf[pos..pos + src.len()].copy_from_slice(src);
    pos + src.len()
}

fn parse_url(s: &str) -> Option<(&str, u16, &str)> {
    let rest = s.strip_prefix("http://")?;
    if rest.is_empty() {
        return None;
    }
    let (hp, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match hp.rfind(':') {
        Some(i) => (&hp[..i], parse_u16(&hp[i + 1..])?),
        None => (hp, 80u16),
    };
    if host.is_empty() {
        return None;
    }
    Some((host, port, path))
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
