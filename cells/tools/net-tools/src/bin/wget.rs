//! wget — HTTP/1.0 file downloader for ViCell.
//!
//! Usage: wget http://HOST[:PORT][/path] <vfs_path>
//!
//! Downloads the URL body and writes it to `<vfs_path>` via typed VFS Write IPC.

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![forbid(unsafe_code)]
extern crate ostd;

use api::dir_handles::ViDirHandle;
use api::ipc::{NetRequest, NetResponse, VfsRequest, VfsResponse, IPC_BUF_SIZE, NET_ERR_DENIED};
use api::syscall::service;
use ostd::io::{print, println};
use ostd::ipc::{service_call_typed_bounded, IpcError};
use ostd::service::provider_tid;
use ostd::syscall::{sys_get_time_ms, sys_yield};

/// service-net refused the request: this Cell holds no network right for it.
const NET_DENIED: &str = "permission denied (no network right)";

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

/// 10 s: one net-service request; covers a `Resolve` the service holds until
/// its own DNS query deadline.
const NET_CALL_TICKS: u64 = 1_000;
/// 10 s: one VFS request, a backing-store write included.
const VFS_CALL_TICKS: u64 = 1_000;

api::declare_syscalls![
    Send,
    Recv,
    Log,
    StateRestore,
    LookupServiceBound,
    VfsMutate,
    GetTime
];

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
    let net_ep = match provider_tid(service::NET) {
        Some(ep) => ep,
        None => {
            println("wget: no net service");
            return;
        }
    };
    let vfs_ep = match provider_tid(service::VFS) {
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
        Ok(a) => a,
        Err(why) => {
            print("wget: ");
            println(why);
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
        Ok(NetResponse::Err(NET_ERR_DENIED)) => {
            print("wget: connect: ");
            println(NET_DENIED);
            return;
        }
        Ok(_) | Err(IpcError::Decode) => {
            println("wget: connect failed");
            return;
        }
        Err(_) => {
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
        let mut cnt_buf = [0u8; IPC_BUF_SIZE];
        let send_req = NetRequest::TcpSend {
            cap_id,
            data: &rem[..chunk],
        };
        match net_call(net_ep, &send_req, &mut cnt_buf) {
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

    // ── Accumulate HTTP response via TcpRecv ──────────────────────────────────
    let mut response = [0u8; RESP_BUF];
    let mut resp_len = 0usize;

    let recv_req = NetRequest::TcpRecv {
        cap_id,
        buf_len: 500,
    };

    'recv: for _ in 0..EXCHANGE_POLL_CEILING {
        let mut data_buf = [0u8; IPC_BUF_SIZE];
        match net_call(net_ep, &recv_req, &mut data_buf) {
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
                    let mut fb = [0u8; IPC_BUF_SIZE];
                    if let Ok(NetResponse::Data(b)) = net_call(net_ep, &recv_req, &mut fb) {
                        let n = b.len().min(RESP_BUF - resp_len);
                        response[resp_len..resp_len + n].copy_from_slice(&b[..n]);
                        resp_len += n;
                    }
                    break 'recv;
                }
                if exchange_expired(started_ms) {
                    break 'recv;
                }
                sys_yield();
            }
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

    // Content cap mirrors the 512-byte IPC frame minus path + envelope.
    let cl = body.len().min(440usize.saturating_sub(vfs_path.len()));
    match save(vfs_ep, vfs_path, &body[..cl]) {
        Ok(()) => {
            ostd::io::print("wget: saved ");
            ostd::io::print_usize(cl);
            ostd::io::print(" bytes to ");
            println(vfs_path);
        }
        Err(why) => {
            print("wget: ");
            println(why);
        }
    }
}

const VFS_WRITE_FAILED: &str = "VFS write failed";

/// Write `content` to `vfs_path`.
///
/// A Cell launched with a cwd grant (every shell-launched foreground child) is
/// sealed by the VFS: path-string requests are refused, and the grant is the
/// only directory it may reach. The file is therefore written by handle,
/// `OpenDir` down from the granted cwd and `WriteAt` the leaf, so `vfs_path`
/// must lie at or below the cwd. A Cell launched without that grant still names
/// the path.
fn save(vfs_ep: usize, vfs_path: &str, content: &[u8]) -> Result<(), &'static str> {
    let context = ostd::child_context::ChildContext::read_own().ok();
    let granted = context
        .as_ref()
        .and_then(|c| Some((c.cwd_grant()?, core::str::from_utf8(c.cwd()).ok()?)));
    let Some((grant, cwd)) = granted else {
        let req = VfsRequest::Write {
            path: vfs_path,
            content,
        };
        return match vfs_call(vfs_ep, &req, &mut [0u8; IPC_BUF_SIZE]) {
            Some(VfsResponse::Ok) => Ok(()),
            _ => Err(VFS_WRITE_FAILED),
        };
    };
    let relative = below(cwd, vfs_path).ok_or("output path is outside the working directory")?;
    let mut parts = relative
        .split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .peekable();
    let mut dir = ViDirHandle(grant.id);
    // First handle derived from the grant; closing it revokes every later one.
    let mut derived = None;
    let result = loop {
        let Some(name) = parts.next() else {
            break Err("output path names no file");
        };
        if parts.peek().is_none() {
            let req = VfsRequest::WriteAt { dir, name, content };
            break match vfs_call(vfs_ep, &req, &mut [0u8; IPC_BUF_SIZE]) {
                Some(VfsResponse::Ok) => Ok(()),
                _ => Err(VFS_WRITE_FAILED),
            };
        }
        let req = VfsRequest::OpenDir { dir, name };
        match vfs_call(vfs_ep, &req, &mut [0u8; IPC_BUF_SIZE]) {
            Some(VfsResponse::DirHandle(sub)) => {
                derived.get_or_insert(sub);
                dir = sub;
            }
            _ => break Err(VFS_WRITE_FAILED),
        }
    };
    if let Some(first) = derived {
        let req = VfsRequest::CloseDir { dir: first };
        let _ = vfs_call(vfs_ep, &req, &mut [0u8; IPC_BUF_SIZE]);
    }
    result
}

/// `target` relative to the directory `cwd`, or `None` when an absolute
/// `target` lies outside it. A relative `target` is already relative to it.
fn below<'a>(cwd: &str, target: &'a str) -> Option<&'a str> {
    if !target.starts_with('/') {
        return Some(target);
    }
    let rest = target.strip_prefix(cwd.trim_end_matches('/'))?;
    (rest.is_empty() || rest.starts_with('/')).then_some(rest)
}

/// One typed VFS request/reply; `None` when it cannot be encoded or decoded,
/// or the VFS died or did not answer within [`VFS_CALL_TICKS`].
fn vfs_call<'b>(
    vfs_ep: usize,
    req: &VfsRequest<'_>,
    resp: &'b mut [u8; IPC_BUF_SIZE],
) -> Option<VfsResponse<'b>> {
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    service_call_typed_bounded(vfs_ep, req, &mut req_buf, resp, VFS_CALL_TICKS).ok()
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
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match net_call(net_ep, &NetRequest::SocketState { cap_id }, &mut resp_buf) {
        Ok(NetResponse::State(s)) => s,
        _ => 0,
    }
}

fn close_socket(cap_id: u32, net_ep: usize) {
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    // An undecodable reply still arrived; only a missing one is reported.
    let reply = net_call(net_ep, &NetRequest::TcpClose { cap_id }, &mut resp_buf);
    if matches!(reply, Err(e) if e != IpcError::Decode) {
        println("wget: socket close reply missing");
    }
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
        Ok(NetResponse::Err(NET_ERR_DENIED)) => Err(NET_DENIED),
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
