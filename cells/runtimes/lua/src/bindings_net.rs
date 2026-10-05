//! Lua `vnet` bindings. All socket ownership and DNS resolution live in `/bin/net`.
#![allow(non_snake_case)] // Lua C API convention

use core::ffi::{c_char, c_int};
use crate::ffi::LuaState;
use api::ipc::{NetRequest, NetResponse, IPC_BUF_SIZE, NET_TCP_INLINE_DATA_MAX};
use ostd::service::NetRef;

const MAX_RECV: usize = 512;
const RETRIES: usize = 500;
const POLL_TIMEOUT_MS: u64 = 3000;

fn poll_pending(start: Option<u64>, attempts: usize) -> bool {
    match start {
        Some(start) => ostd::syscall::sys_get_time_ms()
            .is_some_and(|now| now.saturating_sub(start) < POLL_TIMEOUT_MS),
        None => attempts < RETRIES,
    }
}

// NetRef resolves the live provider and uses service_call_typed -> recv_from,
// which rejects a death notification or input event from another sender.
fn call<'a>(req: &NetRequest<'_>, reply: &'a mut [u8; IPC_BUF_SIZE]) -> Option<NetResponse<'a>> {
    NetRef::new().call(req, reply).ok()
}

/// # Safety
/// `L` is a live Lua state; the stack value at `idx` remains present while borrowed.
unsafe fn arg_bytes<'a>(L: *mut LuaState, idx: c_int) -> Option<&'a [u8]> {
    let mut len = 0;
    let ptr = unsafe { crate::ffi::lua_tolstring(L, idx, &mut len) };
    if ptr.is_null() { None } else { Some(unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len) }) }
}

fn parse_ip(bytes: &[u8]) -> Option<[u8; 4]> {
    let s = core::str::from_utf8(bytes).ok()?;
    let mut parts = s.split('.');
    let mut ip = [0; 4];
    for octet in &mut ip {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) { return None; }
        *octet = part.parse().ok()?;
    }
    if parts.next().is_some() { return None; }
    Some(ip)
}

fn socket_id(L: *mut LuaState, idx: c_int) -> Option<u32> {
    let mut valid = 0;
    let id = unsafe { crate::ffi::lua_tointegerx(L, idx, &mut valid) };
    if valid == 0 || id <= 0 || id > u32::MAX as i64 { None } else { Some(id as u32) }
}

fn port(L: *mut LuaState, idx: c_int) -> Option<u16> {
    let mut valid = 0;
    let n = unsafe { crate::ffi::lua_tointegerx(L, idx, &mut valid) };
    if valid == 0 || !(1..=u16::MAX as i64).contains(&n) { None } else { Some(n as u16) }
}

fn recv_len(L: *mut LuaState, idx: c_int) -> u32 {
    let mut valid = 0;
    let n = unsafe { crate::ffi::lua_tointegerx(L, idx, &mut valid) };
    if valid == 0 { MAX_RECV as u32 } else { n.clamp(1, MAX_RECV as i64) as u32 }
}

fn push_bytes(L: *mut LuaState, bytes: &[u8]) {
    unsafe { crate::ffi::lua_pushlstring(L, bytes.as_ptr().cast::<c_char>(), bytes.len()); }
}
fn push_nil(L: *mut LuaState) -> c_int {
    unsafe { crate::ffi::lua_pushnil(L); }
    1
}
fn push_count(L: *mut LuaState, n: usize) -> c_int {
    unsafe { crate::ffi::lua_pushinteger(L, n as i64); }
    1
}
fn push_ip(L: *mut LuaState, ip: [u8; 4]) {
    let mut text = [0u8; 15];
    let mut pos = 0;
    for (idx, octet) in ip.iter().enumerate() {
        if idx != 0 { text[pos] = b'.'; pos += 1; }
        let mut digits = [0u8; 3];
        let mut n = *octet;
        let mut len = 0;
        loop {
            digits[len] = b'0' + n % 10;
            len += 1;
            n /= 10;
            if n == 0 { break; }
        }
        for digit in digits[..len].iter().rev() { text[pos] = *digit; pos += 1; }
    }
    push_bytes(L, &text[..pos]);
}
fn count(reply: Option<NetResponse<'_>>, limit: usize) -> Option<usize> {
    match reply {
        Some(NetResponse::Data(b)) if b.len() == 4 => {
            let n = u32::from_le_bytes(b.try_into().ok()?) as usize;
            (n <= limit).then_some(n)
        }
        _ => None,
    }
}
fn close(id: u32) {
    let mut buf = [0; IPC_BUF_SIZE];
    let _ = call(&NetRequest::TcpClose { cap_id: id }, &mut buf);
}

/// `vnet.connect(ip, port)` -> cap | nil, error.
pub unsafe extern "C" fn vnet_connect(L: *mut LuaState) -> c_int {
    let ip = unsafe { arg_bytes(L, 1) }.and_then(parse_ip);
    let Some((ip, port)) = ip.zip(port(L, 2)) else {
        unsafe { crate::ffi::lua_pushnil(L); crate::ffi::lua_pushstring(L, c"invalid address".as_ptr()); }
        return 2;
    };
    let mut buf = [0; IPC_BUF_SIZE];
    match call(&NetRequest::TcpConnect { addr: ip, port }, &mut buf) {
        Some(NetResponse::CapId(id)) if id != 0 => push_count(L, id as usize),
        _ => {
            unsafe { crate::ffi::lua_pushnil(L); crate::ffi::lua_pushstring(L, c"connect failed".as_ptr()); }
            2
        }
    }
}

/// `vnet.send(cap, bytes)` -> bytes accepted, including partial writes.
pub unsafe extern "C" fn vnet_send(L: *mut LuaState) -> c_int {
    let Some(id) = socket_id(L, 1) else { return push_count(L, 0); };
    let data = unsafe { arg_bytes(L, 2) }.unwrap_or(&[]);
    let mut sent = 0;
    let mut stalls = 0;
    let start = ostd::syscall::sys_get_time_ms();
    while sent < data.len() && poll_pending(start, stalls) {
        let end = (sent + NET_TCP_INLINE_DATA_MAX).min(data.len());
        let mut buf = [0; IPC_BUF_SIZE];
        match count(call(&NetRequest::TcpSend { cap_id: id, data: &data[sent..end] }, &mut buf), end - sent) {
            Some(0) => { stalls += 1; ostd::task::yield_now(); }
            Some(n) => { sent += n; stalls = 0; }
            None => break,
        }
    }
    push_count(L, sent)
}

/// `vnet.recv(cap [, len])` -> binary-safe data | nil after bounded polling.
pub unsafe extern "C" fn vnet_recv(L: *mut LuaState) -> c_int {
    let Some(id) = socket_id(L, 1) else { return push_nil(L); };
    let len = recv_len(L, 2);
    let start = ostd::syscall::sys_get_time_ms();
    let mut attempts = 0;
    while poll_pending(start, attempts) {
        attempts += 1;
        let mut buf = [0; IPC_BUF_SIZE];
        match call(&NetRequest::TcpRecv { cap_id: id, buf_len: len }, &mut buf) {
            Some(NetResponse::Data(bytes)) if !bytes.is_empty() => { push_bytes(L, bytes); return 1; }
            Some(NetResponse::Data(_)) => ostd::task::yield_now(),
            _ => return push_nil(L),
        }
    }
    push_nil(L)
}

/// `vnet.close(cap)`.
pub unsafe extern "C" fn vnet_close(L: *mut LuaState) -> c_int {
    if let Some(id) = socket_id(L, 1) { close(id); }
    0
}

/// `vnet.udp_socket()` -> cap | nil.
pub unsafe extern "C" fn vnet_udp_socket(L: *mut LuaState) -> c_int {
    let mut buf = [0; IPC_BUF_SIZE];
    match call(&NetRequest::UdpCreate, &mut buf) {
        Some(NetResponse::CapId(id)) if id != 0 => push_count(L, id as usize),
        _ => push_nil(L),
    }
}

/// `vnet.udp_bind(cap, port)` -> boolean.
pub unsafe extern "C" fn vnet_udp_bind(L: *mut LuaState) -> c_int {
    let id = socket_id(L, 1);
    let bind_port = port(L, 2);
    let ok = if let Some((id, bind_port)) = id.zip(bind_port) {
        let mut buf = [0; IPC_BUF_SIZE];
        matches!(call(&NetRequest::UdpBind { cap_id: id, port: bind_port }, &mut buf), Some(NetResponse::Ok))
    } else { false };
    unsafe { crate::ffi::lua_pushboolean(L, ok as c_int); }
    1
}

/// `vnet.udp_send(cap, ip, port, bytes)` -> datagram length or zero.
pub unsafe extern "C" fn vnet_udp_send(L: *mut LuaState) -> c_int {
    let id = socket_id(L, 1);
    let ip = unsafe { arg_bytes(L, 2) }.and_then(parse_ip);
    let destination = port(L, 3);
    let Some(((id, ip), destination)) = id.zip(ip).zip(destination) else { return push_count(L, 0); };
    let data = unsafe { arg_bytes(L, 4) }.unwrap_or(&[]);
    if data.len() > MAX_RECV { return push_count(L, 0); }
    let start = ostd::syscall::sys_get_time_ms();
    let mut attempts = 0;
    while poll_pending(start, attempts) {
        attempts += 1;
        let mut buf = [0; IPC_BUF_SIZE];
        match count(call(&NetRequest::UdpSend { cap_id: id, addr: ip, port: destination, data }, &mut buf), data.len()) {
            Some(0) => ostd::task::yield_now(),
            Some(n) => return push_count(L, n),
            None => break,
        }
    }
    push_count(L, 0)
}

/// `vnet.udp_recv(cap [, len])` -> ip, port, binary-safe datagram | nil.
pub unsafe extern "C" fn vnet_udp_recv(L: *mut LuaState) -> c_int {
    let Some(id) = socket_id(L, 1) else { return push_nil(L); };
    let len = recv_len(L, 2);
    let start = ostd::syscall::sys_get_time_ms();
    let mut attempts = 0;
    while poll_pending(start, attempts) {
        attempts += 1;
        let mut buf = [0; IPC_BUF_SIZE];
        match call(&NetRequest::UdpRecv { cap_id: id, buf_len: len }, &mut buf) {
            Some(NetResponse::Data(bytes)) if bytes.len() >= 6 => {
                push_ip(L, [bytes[0], bytes[1], bytes[2], bytes[3]]);
                push_count(L, u16::from_le_bytes([bytes[4], bytes[5]]) as usize);
                push_bytes(L, &bytes[6..]);
                return 3;
            }
            Some(NetResponse::Data(_)) => ostd::task::yield_now(),
            _ => return push_nil(L),
        }
    }
    push_nil(L)
}

/// `vnet.resolve(name)` -> dotted IPv4 | nil. DNS lives in the net service.
pub unsafe extern "C" fn vnet_resolve(L: *mut LuaState) -> c_int {
    let Some(host) = unsafe { arg_bytes(L, 1) }.and_then(|b| core::str::from_utf8(b).ok()) else { return push_nil(L); };
    let mut buf = [0; IPC_BUF_SIZE];
    match call(&NetRequest::Resolve { hostname: host }, &mut buf) {
        Some(NetResponse::Addr(ip)) => { push_ip(L, ip); 1 }
        _ => push_nil(L),
    }
}
