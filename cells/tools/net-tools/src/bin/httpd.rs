//! httpd — minimal HTTP/1.0 file server for ViCell.
//!
//! Usage: httpd <port> <vfs_path>
//!
//! Listens for TCP connections on <port>.  For each connection, reads the
//! HTTP request (discards it), reads <vfs_path> **fresh from VFS on every
//! request** (no caching), and responds with HTTP/1.0 200 OK + current file
//! content. Returns HTTP 404 when the file is absent, and HTTP 200 with an
//! empty body when the file exists but has zero length. Loops forever,
//! serving one connection at a time.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate alloc;
extern crate ostd;

use alloc::vec::Vec;
use api::ipc::{NetRequest, NetResponse, VfsRequest, VfsResponse, IPC_BUF_SIZE};
use api::syscall::service;
use ostd::clients::VfsClient;
use ostd::io::{print, println};
use ostd::ipc::{service_call_typed, IpcError};
use ostd::syscall::{sys_lookup_service, sys_recv, sys_send, sys_yield, SyscallResult};
use ostd::ViError;

api::declare_syscalls![Send, Recv, Log, StateRestore, LookupService];

// ── Helpers ───────────────────────────────────────────────────────────────────

const MAX_FILE_READ_BYTES: usize = 4096;
const TCP_SEND_MAX_ZERO_PROGRESS_RETRIES: usize = 8;
const NOT_FOUND_RESPONSE: &[u8] =
    b"HTTP/1.0 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 0\r\n\r\n";
const INTERNAL_ERROR_RESPONSE: &[u8] =
    b"HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nContent-Length: 0\r\n\r\n";

enum FileReadOutcome {
    NotFound,
    Ok(Vec<u8>),
    InternalError,
}

enum FileServePlan {
    NotFound,
    Serve(Vec<u8>),
    InternalError,
}

enum StatPreflight {
    NotFound,
    Exists,
    InternalError,
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

fn recv_net_response<'a>(
    net_ep: usize,
    resp_buf: &'a mut [u8; IPC_BUF_SIZE],
) -> Option<NetResponse<'a>> {
    match sys_recv(net_ep, resp_buf) {
        SyscallResult::Ok(sender) if sender == net_ep => {
            api::ipc::decode::<NetResponse>(resp_buf).ok()
        }
        _ => None,
    }
}

fn close_cap(cap: u32, net_ep: usize) {
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let len = api::ipc::encode(&NetRequest::TcpClose { cap_id: cap }, &mut req_buf)
        .map(|b| b.len())
        .unwrap_or(0);
    sys_send(net_ep, &req_buf[..len]);
    let mut r = [0u8; IPC_BUF_SIZE];
    let _ = recv_net_response(net_ep, &mut r);
}

fn query_state(cap: u32, net_ep: usize) -> u8 {
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let len = api::ipc::encode(&NetRequest::SocketState { cap_id: cap }, &mut req_buf)
        .map(|b| b.len())
        .unwrap_or(0);
    sys_send(net_ep, &req_buf[..len]);
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match recv_net_response(net_ep, &mut resp_buf) {
        Some(NetResponse::State(s)) => s,
        _ => 0,
    }
}

fn classify_file_read(result: Result<Vec<u8>, ViError>) -> FileReadOutcome {
    match result {
        Ok(bytes) => FileReadOutcome::Ok(bytes),
        Err(ViError::NotFound) => FileReadOutcome::NotFound,
        Err(_) => FileReadOutcome::InternalError,
    }
}

fn vfs_read(vfs: &mut VfsClient, path: &str) -> FileReadOutcome {
    classify_file_read(vfs.read_file_bounded(path, MAX_FILE_READ_BYTES))
}

fn classify_stat_preflight(resp: Result<VfsResponse<'_>, IpcError>) -> StatPreflight {
    match resp {
        Ok(VfsResponse::Stat { is_dir: true, .. }) => StatPreflight::InternalError,
        Ok(VfsResponse::Stat { is_dir: false, .. }) => StatPreflight::Exists,
        Ok(VfsResponse::Err(1)) => StatPreflight::NotFound,
        Ok(VfsResponse::Err(_)) | Ok(_) | Err(_) => StatPreflight::InternalError,
    }
}

fn plan_file_response(vfs_ep: usize, vfs: &mut VfsClient, path: &str) -> FileServePlan {
    let mut send_buf = [0u8; IPC_BUF_SIZE];
    let mut recv_buf = [0u8; IPC_BUF_SIZE];
    match classify_stat_preflight(service_call_typed::<_, VfsResponse>(
        vfs_ep,
        &VfsRequest::Stat(path),
        &mut send_buf,
        &mut recv_buf,
    )) {
        StatPreflight::NotFound => FileServePlan::NotFound,
        StatPreflight::Exists => match vfs_read(vfs, path) {
            FileReadOutcome::Ok(bytes) => FileServePlan::Serve(bytes),
            FileReadOutcome::NotFound | FileReadOutcome::InternalError => {
                FileServePlan::InternalError
            }
        },
        StatPreflight::InternalError => FileServePlan::InternalError,
    }
}

fn decode_tcp_send_progress(resp: Option<NetResponse<'_>>) -> Option<usize> {
    match resp {
        Some(NetResponse::Data(bytes)) if bytes.len() >= 4 => {
            let mut arr = [0u8; 4];
            arr.copy_from_slice(&bytes[..4]);
            Some(u32::from_le_bytes(arr) as usize)
        }
        _ => None,
    }
}

/// Send bytes to a TCP socket cap via TcpSend, failing if progress stalls or replies are malformed.
fn tcp_send(cap: u32, data: &[u8], net_ep: usize) -> bool {
    let mut sent = 0usize;
    let mut zero_progress_retries = 0usize;

    while sent < data.len() {
        let rem = &data[sent..];
        let chunk = rem.len().min(480);
        let mut send_buf = [0u8; IPC_BUF_SIZE];
        let send_len = api::ipc::encode(
            &NetRequest::TcpSend {
                cap_id: cap,
                data: &rem[..chunk],
            },
            &mut send_buf,
        )
        .map(|b| b.len())
        .unwrap_or(0);
        if send_len == 0 {
            return false;
        }
        sys_send(net_ep, &send_buf[..send_len]);
        let mut cnt_buf = [0u8; IPC_BUF_SIZE];
        match decode_tcp_send_progress(recv_net_response(net_ep, &mut cnt_buf)) {
            Some(0) => {
                zero_progress_retries += 1;
                if zero_progress_retries > TCP_SEND_MAX_ZERO_PROGRESS_RETRIES {
                    return false;
                }
                sys_yield();
            }
            Some(n) if n <= chunk => {
                sent += n;
                zero_progress_retries = 0;
            }
            _ => return false,
        }
    }

    true
}

/// Drain the HTTP request until the header terminator `\r\n\r\n` is seen.
fn drain_request(cap: u32, net_ep: usize) {
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

    for _ in 0..200 {
        sys_send(net_ep, &recv_req_buf[..recv_req_len]);
        let mut data_buf = [0u8; IPC_BUF_SIZE];
        match recv_net_response(net_ep, &mut data_buf) {
            Some(NetResponse::Data(b)) if !b.is_empty() => {
                if b.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Some(NetResponse::Data(_)) => {
                let st = query_state(cap, net_ep);
                if st == 0x06 || st == 0x00 {
                    break;
                }
                sys_yield();
            }
            _ => break,
        }
    }
}

/// Write "Content-Length: N\r\n" as ASCII into `out`.  Returns byte count.
fn write_content_length(n: usize, out: &mut [u8]) -> usize {
    let prefix = b"Content-Length: ";
    let mut pos = 0usize;
    out[pos..pos + prefix.len()].copy_from_slice(prefix);
    pos += prefix.len();
    let mut tmp = [0u8; 20];
    let mut di = 20;
    let mut v = n;
    if v == 0 {
        tmp[19] = b'0';
        di = 19;
    }
    while v > 0 {
        di -= 1;
        tmp[di] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let digits = &tmp[di..];
    out[pos..pos + digits.len()].copy_from_slice(digits);
    pos += digits.len();
    out[pos..pos + 2].copy_from_slice(b"\r\n");
    pos + 2
}

// ── Main ──────────────────────────────────────────────────────────────────────

ostd::cell_main!(cell_main);

fn cell_main() {
    let argv = ostd::args();
    if argv.is_empty() {
        println("Usage: httpd <port> <vfs_path>");
        return;
    }
    let mut parts = argv.iter().map(|arg| arg.as_str());
    let port = match parts.next().and_then(parse_u16) {
        Some(p) => p,
        None => {
            println("Usage: httpd <port> <vfs_path>");
            return;
        }
    };
    let path = match parts.next() {
        Some(p) => p,
        None => {
            println("Usage: httpd <port> <vfs_path>");
            return;
        }
    };

    // ── Resolve service endpoints ─────────────────────────────────────────────
    let net_ep = match sys_lookup_service(service::NET) {
        Some(ep) => ep,
        None => {
            println("httpd: no net service");
            return;
        }
    };
    let vfs_ep = match sys_lookup_service(service::VFS) {
        Some(ep) => ep,
        None => {
            println("httpd: no vfs service");
            return;
        }
    };

    let mut vfs = VfsClient::new();

    // ── TcpListen (atomic create + listen) ───────────────────────────────────
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let len = api::ipc::encode(&NetRequest::TcpListen { port }, &mut req_buf)
        .map(|b| b.len())
        .unwrap_or(0);
    sys_send(net_ep, &req_buf[..len]);
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let listen_cap = match recv_net_response(net_ep, &mut resp_buf) {
        Some(NetResponse::CapId(c)) => c,
        _ => {
            println("httpd: listen failed");
            return;
        }
    };
    print("httpd: listening on ");
    ostd::io::print_usize(port as usize);
    println("");

    // Pre-encode TcpAccept for the listen cap (reused across iterations).
    let mut accept_req_buf = [0u8; IPC_BUF_SIZE];
    let accept_req_len = api::ipc::encode(
        &NetRequest::TcpAccept { cap_id: listen_cap },
        &mut accept_req_buf,
    )
    .map(|b| b.len())
    .unwrap_or(0);

    // Accept loop — serve one connection at a time.
    loop {
        let stream_cap: u32 = loop {
            sys_send(net_ep, &accept_req_buf[..accept_req_len]);
            let mut r = [0u8; IPC_BUF_SIZE];
            match recv_net_response(net_ep, &mut r) {
                Some(NetResponse::CapId(c)) => break c,
                _ => {
                    sys_yield();
                }
            }
        };

        drain_request(stream_cap, net_ep);

        match plan_file_response(vfs_ep, &mut vfs, path) {
            FileServePlan::NotFound => {
                if !tcp_send(stream_cap, NOT_FOUND_RESPONSE, net_ep) {
                    println("httpd: tcp send failed");
                }
            }
            FileServePlan::Serve(bytes) => {
                let mut header = [0u8; 128];
                let mut hlen = 0usize;
                let status = b"HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n";
                header[..status.len()].copy_from_slice(status);
                hlen += status.len();
                hlen += write_content_length(bytes.len(), &mut header[hlen..]);
                header[hlen..hlen + 2].copy_from_slice(b"\r\n");
                hlen += 2;
                if tcp_send(stream_cap, &header[..hlen], net_ep) {
                    if !tcp_send(stream_cap, &bytes, net_ep) {
                        println("httpd: tcp send failed");
                    }
                } else {
                    println("httpd: tcp send failed");
                }
            }
            FileServePlan::InternalError => {
                if !tcp_send(stream_cap, INTERNAL_ERROR_RESPONSE, net_ep) {
                    println("httpd: tcp send failed");
                }
            }
        }

        // Yield to let smoltcp flush TX before sending FIN.
        for _ in 0..500 {
            sys_yield();
        }
        close_cap(stream_cap, net_ep);
    }
}

#[cfg(test)]
#[path = "httpd/tests.rs"]
mod httpd_unit_tests;
