// SPDX-License-Identifier: MIT
//! HTTP client for Ocel over CellOS Net IPC.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use api::ipc::{NetRequest, NetResponse, IPC_BUF_SIZE};
use api::syscall::service;
use ostd::syscall::{sys_lookup_service, sys_recv, sys_send, sys_yield, SyscallResult};

const RESP_BUF: usize = 32768; // 32 KB response buffer

pub fn fetch_http(url: &str) -> Result<String, String> {
    let (host, port, path) = parse_url(url).ok_or_else(|| String::from("Invalid HTTP URL"))?;

    let net_ep = sys_lookup_service(service::NET)
        .ok_or_else(|| String::from("Net service unavailable in CellOS"))?;

    // The net service owns resolution (IPv4 literal, SLIRP alias, DNS A-record).
    let addr =
        resolve_host(host, net_ep).ok_or_else(|| format!("Cannot resolve host: {}", host))?;

    // 1. TcpConnect
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let len = api::ipc::encode(&NetRequest::TcpConnect { addr, port }, &mut req_buf)
        .map(|b| b.len())
        .map_err(|_| String::from("Encode TcpConnect failed"))?;

    sys_send(net_ep, &req_buf[..len]);
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let cap_id = match sys_recv(0, &mut resp_buf) {
        SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&resp_buf) {
            Ok(NetResponse::CapId(c)) => c,
            _ => return Err(String::from("TCP Connection refused or failed")),
        },
        _ => return Err(String::from("Net IPC recv failed on connect")),
    };

    // 2. Build HTTP GET request
    let request_str = format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nUser-Agent: Ocel/0.1 (CellOS)\r\nConnection: close\r\n\r\n",
        path, host
    );
    let request_bytes = request_str.as_bytes();

    // 3. Send HTTP request
    let mut sent = 0usize;
    for _ in 0..500 {
        if sent >= request_bytes.len() {
            break;
        }
        let rem = &request_bytes[sent..];
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
        .map_err(|_| String::from("Encode TcpSend failed"))?;

        sys_send(net_ep, &send_buf[..send_len]);
        let mut cnt_buf = [0u8; IPC_BUF_SIZE];
        match sys_recv(0, &mut cnt_buf) {
            SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&cnt_buf) {
                Ok(NetResponse::Data(b)) if b.len() >= 4 => {
                    let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
                    sent += n;
                    if n == 0 {
                        sys_yield();
                    }
                }
                _ => break,
            },
            _ => break,
        }
    }

    // 4. Accumulate HTTP response
    let mut response: Vec<u8> = Vec::with_capacity(4096);
    let mut recv_req_buf = [0u8; IPC_BUF_SIZE];
    let recv_req_len = api::ipc::encode(
        &NetRequest::TcpRecv {
            cap_id,
            buf_len: 500,
        },
        &mut recv_req_buf,
    )
    .map(|b| b.len())
    .map_err(|_| String::from("Encode TcpRecv failed"))?;

    for _ in 0..1000 {
        sys_send(net_ep, &recv_req_buf[..recv_req_len]);
        let mut data_buf = [0u8; IPC_BUF_SIZE];
        match sys_recv(0, &mut data_buf) {
            SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&data_buf) {
                Ok(NetResponse::Data(b)) if !b.is_empty() => {
                    response.extend_from_slice(b);
                    if response.len() >= RESP_BUF {
                        break;
                    }
                }
                Ok(NetResponse::Data(_)) => {
                    let st = query_state(cap_id, net_ep);
                    if st == 0x06 || st == 0x00 {
                        // CloseWait or Closed -> final recv
                        sys_send(net_ep, &recv_req_buf[..recv_req_len]);
                        let mut fb = [0u8; IPC_BUF_SIZE];
                        if let SyscallResult::Ok(_) = sys_recv(0, &mut fb) {
                            if let Ok(NetResponse::Data(b)) = api::ipc::decode::<NetResponse>(&fb) {
                                response.extend_from_slice(b);
                            }
                        }
                        break;
                    }
                    sys_yield();
                }
                _ => break,
            },
            _ => break,
        }
    }

    close_socket(cap_id, net_ep);

    if response.is_empty() {
        return Err(String::from("Empty HTTP response from server"));
    }

    // 5. Separate HTTP Headers and Body
    if let Some(header_end) = response.windows(4).position(|w| w == b"\r\n\r\n") {
        let body = &response[header_end + 4..];
        String::from_utf8(body.to_vec())
            .map_err(|_| String::from("HTTP response contains non-UTF8 data"))
    } else {
        String::from_utf8(response)
            .map_err(|_| String::from("HTTP response contains non-UTF8 data"))
    }
}

pub fn fetch_https(url: &str) -> Result<String, String> {
    let (host, port, path) =
        parse_url_scheme(url, "https://", 443).ok_or_else(|| String::from("Invalid HTTPS URL"))?;

    let net_ep = sys_lookup_service(service::NET)
        .ok_or_else(|| String::from("Net service unavailable in CellOS"))?;

    // The net service owns resolution (IPv4 literal, SLIRP alias, DNS A-record).
    let addr =
        resolve_host(host, net_ep).ok_or_else(|| format!("Cannot resolve host: {}", host))?;

    // 1. Open TLS 1.3 connection via ostd::tls
    let cap_id = ostd::tls::tls_connect(net_ep, addr, port, host);
    if cap_id == 0 {
        return Err(format!(
            "TLS 1.3 handshake failed connecting to {}:{}",
            host, port
        ));
    }

    // 2. Build HTTP GET request
    let request_str = format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nUser-Agent: Ocel/0.1 (CellOS)\r\nConnection: close\r\n\r\n",
        path, host
    );
    let request_bytes = request_str.as_bytes();

    // 3. Send over TLS
    let mut sent = 0usize;
    for _ in 0..500 {
        if sent >= request_bytes.len() {
            break;
        }
        let chunk = &request_bytes[sent..];
        let n = ostd::tls::tls_write(net_ep, cap_id, chunk);
        if n > 0 {
            sent += n;
        } else {
            sys_yield();
        }
    }

    // 4. Read decrypted response over TLS
    let mut response: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk_buf = [0u8; 1024];

    for _ in 0..500 {
        let n = ostd::tls::tls_read(net_ep, cap_id, &mut chunk_buf);
        if n > 0 {
            response.extend_from_slice(&chunk_buf[..n]);
            if response.len() >= RESP_BUF {
                break;
            }
        } else {
            sys_yield();
        }
    }

    ostd::tls::tls_close(net_ep, cap_id);

    if response.is_empty() {
        return Err(String::from("Empty response from HTTPS server"));
    }

    // 5. Separate HTTP headers and body
    if let Some(header_end) = response.windows(4).position(|w| w == b"\r\n\r\n") {
        let body = &response[header_end + 4..];
        String::from_utf8(body.to_vec())
            .map_err(|_| String::from("HTTPS response contains non-UTF8 data"))
    } else {
        String::from_utf8(response)
            .map_err(|_| String::from("HTTPS response contains non-UTF8 data"))
    }
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
            Ok(NetResponse::Data(b)) if !b.is_empty() => b[0],
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

fn parse_url(s: &str) -> Option<(&str, u16, &str)> {
    parse_url_scheme(s, "http://", 80)
}

fn parse_url_scheme<'a>(
    s: &'a str,
    scheme: &str,
    default_port: u16,
) -> Option<(&'a str, u16, &'a str)> {
    let rest = s.strip_prefix(scheme)?;
    if rest.is_empty() {
        return None;
    }
    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match host_port.rfind(':') {
        Some(i) => (&host_port[..i], parse_u16(&host_port[i + 1..])?),
        None => (host_port, default_port),
    };
    if host.is_empty() {
        return None;
    }
    Some((host, port, path))
}

/// Resolve `host` through the net service (`NetRequest::Resolve`), so the
/// browser shares the service's literal/alias/DNS order instead of a copy.
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
