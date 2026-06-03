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

const SOCKET_TCP: u8 = 0x10;
const CONNECT:    u8 = 0x12;
const SEND_OP:    u8 = 0x13;
const RECV_OP:    u8 = 0x14;
const CLOSE_OP:   u8 = 0x15;

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
