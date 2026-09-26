//! Rust-side TCP socket bindings exposed to Lua via C FFI (`vnet.*`).
// `L` is the universal Lua C API convention for `lua_State*`.
#![allow(non_snake_case)] // reason: L is the Lua C API convention for lua_State pointers
//!
//! Mirrors the verified IPC wire format used by `nc.rs`: every message is
//! `[opcode:1][cap:8 LE][payload:*]` sent to the net service (endpoint 6).
//! Replies are read with `sys_recv`, which returns the SENDER id, not a byte
//! count — reply length is bounded by the buffer we pass.

extern crate alloc;

use core::ffi::{c_char, c_int};
use crate::ffi::LuaState;
use ostd::syscall::{sys_recv, sys_send, sys_yield, SyscallResult};

/// Net service cell task ID (init spawn order: vfs=3, config=4, input=5, net=6).
const NET_ENDPOINT: usize = 6;

const SOCKET_TCP:  u8 = 0x10;
const SOCKET_UDP:  u8 = 0x11;
const CONNECT:     u8 = 0x12;
const SEND_OP:     u8 = 0x13;
const RECV_OP:     u8 = 0x14;
const CLOSE_OP:    u8 = 0x15;
const BIND_OP:     u8 = 0x16;
const SENDTO_OP:   u8 = 0x21;
const RECVFROM_OP: u8 = 0x22;

/// Upper bound for a single SEND payload copied off the Lua stack.
const MAX_SEND: usize = 512;
/// Upper bound for a RECV request (matches net cell's 4096 recv cap).
const MAX_RECV: usize = 4096;

/// Read the string arg at stack `idx` as a byte slice borrowed from Lua.
///
/// # Safety
/// `L` must be valid; the returned slice lives only while the value stays on
/// the Lua stack (caller must not pop before use).
unsafe fn lua_arg_bytes<'a>(L: *mut LuaState, idx: c_int) -> Option<&'a [u8]> {
    let mut len: usize = 0;
    // SAFETY: L valid; idx is a checked stack position.
    let ptr = unsafe { crate::ffi::lua_tolstring(L, idx, &mut len as *mut _) };
    if ptr.is_null() { return None; }
    // SAFETY: Lua guarantees `len` valid bytes at `ptr`.
    Some(unsafe { core::slice::from_raw_parts(ptr as *const u8, len) })
}

/// Parse "a.b.c.d" into 4 octets.
fn parse_ipv4(s: &[u8]) -> Option<[u8; 4]> {
    let s = core::str::from_utf8(s).ok()?;
    let mut it = s.splitn(5, '.');
    let mut out = [0u8; 4];
    for slot in out.iter_mut() {
        let part = it.next()?;
        let mut n: u16 = 0;
        if part.is_empty() { return None; }
        for ch in part.bytes() {
            if !(b'0'..=b'9').contains(&ch) { return None; }
            n = n * 10 + (ch - b'0') as u16;
            if n > 255 { return None; }
        }
        *slot = n as u8;
    }
    if it.next().is_some() { return None; }
    Some(out)
}

/// `vnet.connect(ip_str, port_int)` → cap_id | nil, errmsg
#[no_mangle]
pub unsafe extern "C" fn vnet_connect(L: *mut LuaState) -> c_int {
    // SAFETY: L valid; arg 1 is the ip string, arg 2 the port integer.
    let ip = match unsafe { lua_arg_bytes(L, 1) }.and_then(parse_ipv4) {
        Some(a) => a,
        None => {
            unsafe { crate::ffi::lua_pushnil(L) };
            unsafe { crate::ffi::lua_pushstring(L, c"invalid ip".as_ptr()) };
            return 2;
        }
    };
    let port = unsafe { crate::ffi::lua_tointegerx(L, 2, core::ptr::null_mut()) } as u16;

    // SOCKET_TCP → cap
    let socket_msg = [SOCKET_TCP, 0, 0, 0, 0, 0, 0, 0, 0];
    sys_send(NET_ENDPOINT, &socket_msg);
    let mut cap_reply = [0u8; 8];
    let cap = match sys_recv(0, &mut cap_reply) {
        SyscallResult::Ok(_) => u64::from_le_bytes(cap_reply),
        _ => 0,
    };
    if cap == 0 {
        unsafe { crate::ffi::lua_pushnil(L) };
        unsafe { crate::ffi::lua_pushstring(L, c"socket failed".as_ptr()) };
        return 2;
    }

    // CONNECT [0x12][cap:8][addr:4][port:2 LE]
    let mut conn = [0u8; 15];
    conn[0] = CONNECT;
    conn[1..9].copy_from_slice(&cap.to_le_bytes());
    conn[9..13].copy_from_slice(&ip);
    conn[13..15].copy_from_slice(&port.to_le_bytes());
    sys_send(NET_ENDPOINT, &conn);
    let mut ack = [0u8; 1];
    match sys_recv(0, &mut ack) {
        SyscallResult::Ok(_) if ack[0] == 0x00 => {
            // SAFETY: L valid; cap fits in i64.
            unsafe { crate::ffi::lua_pushinteger(L, cap as i64) };
            1
        }
        _ => {
            unsafe { crate::ffi::lua_pushnil(L) };
            unsafe { crate::ffi::lua_pushstring(L, c"connect failed".as_ptr()) };
            2
        }
    }
}

/// `vnet.send(cap_id, data_str)` → bytes_written
#[no_mangle]
pub unsafe extern "C" fn vnet_send(L: *mut LuaState) -> c_int {
    let cap = unsafe { crate::ffi::lua_tointegerx(L, 1, core::ptr::null_mut()) } as u64;
    // SAFETY: L valid; arg 2 is the data string.
    let raw = unsafe { lua_arg_bytes(L, 2) }.unwrap_or(&[]);
    let data = &raw[..raw.len().min(MAX_SEND)];

    // Retry until all bytes buffered (mirrors nc.rs). Each retry forwards only
    // the unsent suffix so a partial write never duplicates a prefix.
    let mut sent = 0usize;
    for _ in 0..500 {
        if sent >= data.len() { break; }
        let rem = &data[sent..];
        let mut msg = alloc::vec![0u8; 9 + rem.len()];
        msg[0] = SEND_OP;
        msg[1..9].copy_from_slice(&cap.to_le_bytes());
        msg[9..9 + rem.len()].copy_from_slice(rem);
        sys_send(NET_ENDPOINT, &msg);
        let mut cnt = [0u8; 4];
        match sys_recv(0, &mut cnt) {
            SyscallResult::Ok(_) => {
                let n = u32::from_le_bytes(cnt) as usize;
                sent += n;
                if n == 0 { sys_yield(); }
            }
            _ => break,
        }
    }
    // SAFETY: L valid.
    unsafe { crate::ffi::lua_pushinteger(L, sent as i64) };
    1
}

/// `vnet.recv(cap_id [, buf_len])` → data_str | nil
///
/// Polls until data arrives (up to 500 retries). Trims at the first NUL byte
/// because `sys_recv` returns sender_id, not byte count — ASCII-only payloads.
#[no_mangle]
pub unsafe extern "C" fn vnet_recv(L: *mut LuaState) -> c_int {
    let cap = unsafe { crate::ffi::lua_tointegerx(L, 1, core::ptr::null_mut()) } as u64;
    let mut isnum: c_int = 0;
    let req = unsafe { crate::ffi::lua_tointegerx(L, 2, &mut isnum as *mut _) };
    let buf_len = if isnum != 0 { (req as usize).min(MAX_RECV) } else { 512 };

    let mut recv_msg = [0u8; 13];
    recv_msg[0] = RECV_OP;
    recv_msg[1..9].copy_from_slice(&cap.to_le_bytes());
    recv_msg[9..13].copy_from_slice(&(buf_len as u32).to_le_bytes());

    let mut data = alloc::vec![0u8; buf_len];
    for _ in 0..500 {
        // Zero before each receive so a short reply leaves no stale tail.
        for b in data.iter_mut() { *b = 0; }
        sys_send(NET_ENDPOINT, &recv_msg);
        match sys_recv(0, &mut data) {
            SyscallResult::Ok(_) if data[0] != 0 => {
                // Trim at first NUL — sys_recv length unknown (returns sender_id).
                let end = data.iter().position(|&b| b == 0).unwrap_or(buf_len);
                // SAFETY: L valid; data[..end] is initialised bytes.
                unsafe {
                    crate::ffi::lua_pushlstring(L, data.as_ptr() as *const c_char, end);
                }
                return 1;
            }
            _ => sys_yield(),
        }
    }
    // SAFETY: L valid.
    unsafe { crate::ffi::lua_pushnil(L) };
    1
}

/// `vnet.close(cap_id)` — no return value.
#[no_mangle]
pub unsafe extern "C" fn vnet_close(L: *mut LuaState) -> c_int {
    let cap = unsafe { crate::ffi::lua_tointegerx(L, 1, core::ptr::null_mut()) } as u64;
    let mut msg = [0u8; 9];
    msg[0] = CLOSE_OP;
    msg[1..9].copy_from_slice(&cap.to_le_bytes());
    sys_send(NET_ENDPOINT, &msg);
    let mut r = [0u8; 1];
    let _ = sys_recv(0, &mut r);
    let _ = L; // no values pushed
    0
}

// ── UDP socket bindings ───────────────────────────────────────────────────────

/// Close a socket cap (internal helper, not exported to Lua).
fn close_cap(cap: u64) {
    let mut msg = [0u8; 9];
    msg[0] = CLOSE_OP;
    msg[1..9].copy_from_slice(&cap.to_le_bytes());
    sys_send(NET_ENDPOINT, &msg);
    let mut r = [0u8; 1];
    let _ = sys_recv(0, &mut r);
}

/// `vnet.udp_send(cap_id, ip_str, port_int, data_str)` → bytes_sent
///
/// Sends one UDP datagram to the specified remote endpoint. Retries only when
/// the TX ring is full (n==0 reply). UDP datagrams are atomic — no offset tracking.
#[no_mangle]
pub unsafe extern "C" fn vnet_udp_send(L: *mut LuaState) -> c_int {
    let cap = unsafe { crate::ffi::lua_tointegerx(L, 1, core::ptr::null_mut()) } as u64;
    // SAFETY: L valid; arg 2 is the destination IP string.
    let ip = match unsafe { lua_arg_bytes(L, 2) }.and_then(parse_ipv4) {
        Some(a) => a,
        None => {
            unsafe { crate::ffi::lua_pushinteger(L, 0) };
            return 1;
        }
    };
    let port = unsafe { crate::ffi::lua_tointegerx(L, 3, core::ptr::null_mut()) } as u16;
    // SAFETY: L valid; arg 4 is the data string.
    let raw = unsafe { lua_arg_bytes(L, 4) }.unwrap_or(&[]);
    let data = &raw[..raw.len().min(MAX_SEND)];

    // Build: [SENDTO_OP][cap:8][addr:4][port:2 LE][data:*]
    let mut msg = alloc::vec![0u8; 9 + 6 + data.len()];
    msg[0] = SENDTO_OP;
    msg[1..9].copy_from_slice(&cap.to_le_bytes());
    msg[9..13].copy_from_slice(&ip);
    msg[13..15].copy_from_slice(&port.to_le_bytes());
    msg[15..15 + data.len()].copy_from_slice(data);

    let mut sent = 0usize;
    for _ in 0..500 {
        sys_send(NET_ENDPOINT, &msg);
        let mut cnt = [0u8; 4];
        match sys_recv(0, &mut cnt) {
            SyscallResult::Ok(_) => {
                let n = u32::from_le_bytes(cnt) as usize;
                if n > 0 { sent = n; break; }
                sys_yield(); // TX buffer full — retry
            }
            _ => break,
        }
    }
    // SAFETY: L valid.
    unsafe { crate::ffi::lua_pushinteger(L, sent as i64) };
    1
}

/// `vnet.udp_recv(cap_id [, buf_len])` → (src_ip, src_port, data) | nil
///
/// Polls for one UDP datagram. On arrival returns 3 values: source IP string,
/// source port integer, and data string. Returns nil on timeout.
#[no_mangle]
pub unsafe extern "C" fn vnet_udp_recv(L: *mut LuaState) -> c_int {
    let cap = unsafe { crate::ffi::lua_tointegerx(L, 1, core::ptr::null_mut()) } as u64;
    let mut isnum: c_int = 0;
    let req = unsafe { crate::ffi::lua_tointegerx(L, 2, &mut isnum as *mut _) };
    let buf_len = if isnum != 0 { (req as usize).min(512) } else { 512 };

    let mut recv_msg = [0u8; 13];
    recv_msg[0] = RECVFROM_OP;
    recv_msg[1..9].copy_from_slice(&cap.to_le_bytes());
    recv_msg[9..13].copy_from_slice(&(buf_len as u32).to_le_bytes());

    // 6-byte header + payload; pre-zero so empty-reply detection is reliable.
    let mut buf = alloc::vec![0u8; 6 + buf_len];
    for _ in 0..500 {
        for b in buf.iter_mut() { *b = 0; }
        sys_send(NET_ENDPOINT, &recv_msg);
        match sys_recv(0, &mut buf) {
            // buf[0] != 0 means a datagram arrived (src IP first byte is non-zero
            // for any real remote, e.g. 10.x = 0x0A).
            SyscallResult::Ok(_) if buf[0] != 0 => {
                let src_ip = [buf[0], buf[1], buf[2], buf[3]];
                let src_port = u16::from_le_bytes([buf[4], buf[5]]);
                // Data starts at byte 6; find end by first NUL (ASCII-safe).
                let data_end = 6 + buf[6..].iter().position(|&b| b == 0)
                    .unwrap_or(buf.len() - 6);
                let mut ip_str = [0u8; 16];
                let ip_len = format_ip(src_ip, &mut ip_str);
                // SAFETY: L valid; ip_str / buf slices are initialised.
                unsafe {
                    crate::ffi::lua_pushlstring(L, ip_str.as_ptr() as *const c_char, ip_len);
                    crate::ffi::lua_pushinteger(L, src_port as i64);
                    crate::ffi::lua_pushlstring(L, buf[6..].as_ptr() as *const c_char, data_end - 6);
                }
                return 3;
            }
            _ => sys_yield(),
        }
    }
    // SAFETY: L valid.
    unsafe { crate::ffi::lua_pushnil(L) };
    1
}

/// `vnet.resolve(hostname)` → ip_str | nil
///
/// One `NetRequest::Resolve` round-trip: the net service owns the whole
/// resolution order (IPv4 literal, SLIRP alias, UDP A-record query to the
/// server the DHCP lease named), so Lua inherits the same answers as the
/// shell tools instead of running a second resolver.
#[no_mangle]
pub unsafe extern "C" fn vnet_resolve(L: *mut LuaState) -> c_int {
    // SAFETY: L valid; arg 1 is the hostname string.
    let raw = match unsafe { lua_arg_bytes(L, 1) } {
        Some(b) => b,
        None => {
            unsafe { crate::ffi::lua_pushnil(L) };
            return 1;
        }
    };
    let hostname = match core::str::from_utf8(raw) {
        Ok(s) => s,
        Err(_) => {
            unsafe { crate::ffi::lua_pushnil(L) };
            return 1;
        }
    };

    let mut request = [0u8; api::ipc::IPC_BUF_SIZE];
    let len = match api::ipc::encode(&api::ipc::NetRequest::Resolve { hostname }, &mut request) {
        Ok(bytes) => bytes.len(),
        Err(_) => {
            unsafe { crate::ffi::lua_pushnil(L) };
            return 1;
        }
    };
    sys_send(NET_ENDPOINT, &request[..len]);

    let mut reply = [0u8; api::ipc::IPC_BUF_SIZE];
    let ip = match sys_recv(0, &mut reply) {
        SyscallResult::Ok(_) => match api::ipc::decode::<api::ipc::NetResponse>(&reply) {
            Ok(api::ipc::NetResponse::Addr(addr)) => Some(addr),
            _ => None,
        },
        _ => None,
    };

    match ip {
        Some(ip) => { push_ip(L, ip); 1 }
        None     => { unsafe { crate::ffi::lua_pushnil(L) }; 1 }
    }
}

/// Format a 4-byte IPv4 address into dotted-decimal in `buf`. Returns byte count.
fn format_ip(ip: [u8; 4], buf: &mut [u8]) -> usize {
    let mut pos = 0;
    for (i, &octet) in ip.iter().enumerate() {
        if i > 0 { buf[pos] = b'.'; pos += 1; }
        let mut n = octet as u32;
        let mut tmp = [0u8; 3];
        let mut di = 3;
        loop { di -= 1; tmp[di] = b'0' + (n % 10) as u8; n /= 10; if n == 0 { break; } }
        let digits = &tmp[di..];
        buf[pos..pos + digits.len()].copy_from_slice(digits);
        pos += digits.len();
    }
    pos
}

/// Push a dotted-decimal IPv4 string onto the Lua stack.
fn push_ip(L: *mut LuaState, ip: [u8; 4]) {
    let mut buf = [0u8; 16];
    let len = format_ip(ip, &mut buf);
    // SAFETY: L valid; buf[..len] is ASCII, no NUL.
    unsafe { crate::ffi::lua_pushlstring(L, buf.as_ptr() as *const c_char, len); }
}
