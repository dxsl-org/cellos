//! Lua `vnet` bindings. All socket ownership and DNS resolution live in `/bin/net`.
#![allow(non_snake_case)] // Lua C API convention

use crate::ffi::LuaState;
use core::ffi::{c_char, c_int, CStr};
use ostd::service::NetRef;
use wire::ipc::{NetRequest, NetResponse, IPC_BUF_SIZE, NET_TCP_INLINE_DATA_MAX};

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

// NetRef resolves the live provider and uses an operation-correlated RPC;
// queued input and unrelated death notifications are not reply completions.
fn call<'a>(req: &NetRequest<'_>, reply: &'a mut [u8; IPC_BUF_SIZE]) -> Option<NetResponse<'a>> {
    NetRef::new().call(req, reply).into_result().ok()
}

/// Stack of the Lua C callback currently executing.
///
/// Only reads argument slots and pushes results; it never pops or replaces a
/// slot, so argument strings stay referenced by the stack while borrowed.
struct LuaStack(*mut LuaState);

impl LuaStack {
    /// # Safety
    /// `L` is the live state passed to the currently executing Lua C callback.
    /// The value must not outlive that callback, and nothing else may pop or
    /// replace the callback's stack slots while it is alive.
    unsafe fn new(L: *mut LuaState) -> Self {
        Self(L)
    }

    fn arg_bytes(&self, idx: c_int) -> Option<&[u8]> {
        let mut len = 0;
        // SAFETY: `self.0` is live per `new`; `len` is a valid out-pointer.
        let ptr = unsafe { crate::ffi::lua_tolstring(self.0, idx, &mut len) };
        if ptr.is_null() {
            None
        } else {
            // SAFETY: Lua returned `len` bytes owned by the string in slot
            // `idx`; the slot is never popped while `self` is borrowed.
            Some(unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len) })
        }
    }

    fn integer(&self, idx: c_int) -> Option<i64> {
        let mut valid = 0;
        // SAFETY: `self.0` is live per `new`; `valid` is a valid out-pointer.
        let n = unsafe { crate::ffi::lua_tointegerx(self.0, idx, &mut valid) };
        (valid != 0).then_some(n)
    }

    fn socket_id(&self, idx: c_int) -> Option<u32> {
        let id = self.integer(idx)?;
        if id <= 0 || id > u32::MAX as i64 {
            None
        } else {
            Some(id as u32)
        }
    }

    fn port(&self, idx: c_int) -> Option<u16> {
        let n = self.integer(idx)?;
        if !(1..=u16::MAX as i64).contains(&n) {
            None
        } else {
            Some(n as u16)
        }
    }

    fn recv_len(&self, idx: c_int) -> u32 {
        match self.integer(idx) {
            None => MAX_RECV as u32,
            Some(n) => n.clamp(1, MAX_RECV as i64) as u32,
        }
    }

    fn push_bytes(&self, bytes: &[u8]) {
        // SAFETY: `self.0` is live per `new`; Lua copies `bytes` before returning.
        unsafe {
            crate::ffi::lua_pushlstring(self.0, bytes.as_ptr().cast::<c_char>(), bytes.len());
        }
    }

    fn push_nil(&self) -> c_int {
        // SAFETY: `self.0` is live per `new`.
        unsafe {
            crate::ffi::lua_pushnil(self.0);
        }
        1
    }

    fn push_count(&self, n: usize) -> c_int {
        // SAFETY: `self.0` is live per `new`.
        unsafe {
            crate::ffi::lua_pushinteger(self.0, n as i64);
        }
        1
    }

    fn push_bool(&self, b: bool) -> c_int {
        // SAFETY: `self.0` is live per `new`.
        unsafe {
            crate::ffi::lua_pushboolean(self.0, b as c_int);
        }
        1
    }

    /// Pushes `nil, msg`.
    fn push_error(&self, msg: &CStr) -> c_int {
        self.push_nil();
        // SAFETY: `self.0` is live per `new`; `msg` is NUL-terminated and Lua
        // copies it before returning.
        unsafe {
            crate::ffi::lua_pushstring(self.0, msg.as_ptr());
        }
        2
    }

    fn push_ip(&self, ip: [u8; 4]) {
        let mut text = [0u8; 15];
        let mut pos = 0;
        for (idx, octet) in ip.iter().enumerate() {
            if idx != 0 {
                text[pos] = b'.';
                pos += 1;
            }
            let mut digits = [0u8; 3];
            let mut n = *octet;
            let mut len = 0;
            loop {
                digits[len] = b'0' + n % 10;
                len += 1;
                n /= 10;
                if n == 0 {
                    break;
                }
            }
            for digit in digits[..len].iter().rev() {
                text[pos] = *digit;
                pos += 1;
            }
        }
        self.push_bytes(&text[..pos]);
    }
}

fn parse_ip(bytes: &[u8]) -> Option<[u8; 4]> {
    let s = core::str::from_utf8(bytes).ok()?;
    let mut parts = s.split('.');
    let mut ip = [0; 4];
    for octet in &mut ip {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *octet = part.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(ip)
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
    // SAFETY: Lua passes its live state for the duration of this callback.
    let lua = unsafe { LuaStack::new(L) };
    let ip = lua.arg_bytes(1).and_then(parse_ip);
    let Some((ip, port)) = ip.zip(lua.port(2)) else {
        return lua.push_error(c"invalid address");
    };
    let mut buf = [0; IPC_BUF_SIZE];
    match call(&NetRequest::TcpConnect { addr: ip, port }, &mut buf) {
        Some(NetResponse::CapId(id)) if id != 0 => lua.push_count(id as usize),
        _ => lua.push_error(c"connect failed"),
    }
}

/// `vnet.send(cap, bytes)` -> bytes accepted, including partial writes.
pub unsafe extern "C" fn vnet_send(L: *mut LuaState) -> c_int {
    // SAFETY: Lua passes its live state for the duration of this callback.
    let lua = unsafe { LuaStack::new(L) };
    let Some(id) = lua.socket_id(1) else {
        return lua.push_count(0);
    };
    let data = lua.arg_bytes(2).unwrap_or(&[]);
    let mut sent = 0;
    let mut stalls = 0;
    let start = ostd::syscall::sys_get_time_ms();
    while sent < data.len() && poll_pending(start, stalls) {
        let end = (sent + NET_TCP_INLINE_DATA_MAX).min(data.len());
        let mut buf = [0; IPC_BUF_SIZE];
        match count(
            call(
                &NetRequest::TcpSend {
                    cap_id: id,
                    data: &data[sent..end],
                },
                &mut buf,
            ),
            end - sent,
        ) {
            Some(0) => {
                stalls += 1;
                ostd::task::yield_now();
            }
            Some(n) => {
                sent += n;
                stalls = 0;
            }
            None => break,
        }
    }
    lua.push_count(sent)
}

/// `vnet.recv(cap [, len])` -> binary-safe data | nil after bounded polling.
pub unsafe extern "C" fn vnet_recv(L: *mut LuaState) -> c_int {
    // SAFETY: Lua passes its live state for the duration of this callback.
    let lua = unsafe { LuaStack::new(L) };
    let Some(id) = lua.socket_id(1) else {
        return lua.push_nil();
    };
    let len = lua.recv_len(2);
    let start = ostd::syscall::sys_get_time_ms();
    let mut attempts = 0;
    while poll_pending(start, attempts) {
        attempts += 1;
        let mut buf = [0; IPC_BUF_SIZE];
        match call(
            &NetRequest::TcpRecv {
                cap_id: id,
                buf_len: len,
            },
            &mut buf,
        ) {
            Some(NetResponse::Data(bytes)) if !bytes.is_empty() => {
                lua.push_bytes(bytes);
                return 1;
            }
            Some(NetResponse::Data(_)) => ostd::task::yield_now(),
            _ => return lua.push_nil(),
        }
    }
    lua.push_nil()
}

/// `vnet.close(cap)`.
pub unsafe extern "C" fn vnet_close(L: *mut LuaState) -> c_int {
    // SAFETY: Lua passes its live state for the duration of this callback.
    let lua = unsafe { LuaStack::new(L) };
    if let Some(id) = lua.socket_id(1) {
        close(id);
    }
    0
}

/// `vnet.udp_socket()` -> cap | nil.
pub unsafe extern "C" fn vnet_udp_socket(L: *mut LuaState) -> c_int {
    // SAFETY: Lua passes its live state for the duration of this callback.
    let lua = unsafe { LuaStack::new(L) };
    let mut buf = [0; IPC_BUF_SIZE];
    match call(&NetRequest::UdpCreate, &mut buf) {
        Some(NetResponse::CapId(id)) if id != 0 => lua.push_count(id as usize),
        _ => lua.push_nil(),
    }
}

/// `vnet.udp_bind(cap, port)` -> boolean.
pub unsafe extern "C" fn vnet_udp_bind(L: *mut LuaState) -> c_int {
    // SAFETY: Lua passes its live state for the duration of this callback.
    let lua = unsafe { LuaStack::new(L) };
    let id = lua.socket_id(1);
    let bind_port = lua.port(2);
    let ok = if let Some((id, bind_port)) = id.zip(bind_port) {
        let mut buf = [0; IPC_BUF_SIZE];
        matches!(
            call(
                &NetRequest::UdpBind {
                    cap_id: id,
                    port: bind_port
                },
                &mut buf
            ),
            Some(NetResponse::Ok)
        )
    } else {
        false
    };
    lua.push_bool(ok)
}

/// `vnet.udp_send(cap, ip, port, bytes)` -> datagram length or zero.
pub unsafe extern "C" fn vnet_udp_send(L: *mut LuaState) -> c_int {
    // SAFETY: Lua passes its live state for the duration of this callback.
    let lua = unsafe { LuaStack::new(L) };
    let id = lua.socket_id(1);
    let ip = lua.arg_bytes(2).and_then(parse_ip);
    let destination = lua.port(3);
    let Some(((id, ip), destination)) = id.zip(ip).zip(destination) else {
        return lua.push_count(0);
    };
    let data = lua.arg_bytes(4).unwrap_or(&[]);
    if data.len() > MAX_RECV {
        return lua.push_count(0);
    }
    let start = ostd::syscall::sys_get_time_ms();
    let mut attempts = 0;
    while poll_pending(start, attempts) {
        attempts += 1;
        let mut buf = [0; IPC_BUF_SIZE];
        match count(
            call(
                &NetRequest::UdpSend {
                    cap_id: id,
                    addr: ip,
                    port: destination,
                    data,
                },
                &mut buf,
            ),
            data.len(),
        ) {
            Some(0) => ostd::task::yield_now(),
            Some(n) => return lua.push_count(n),
            None => break,
        }
    }
    lua.push_count(0)
}

/// `vnet.udp_recv(cap [, len])` -> ip, port, binary-safe datagram | nil.
pub unsafe extern "C" fn vnet_udp_recv(L: *mut LuaState) -> c_int {
    // SAFETY: Lua passes its live state for the duration of this callback.
    let lua = unsafe { LuaStack::new(L) };
    let Some(id) = lua.socket_id(1) else {
        return lua.push_nil();
    };
    let len = lua.recv_len(2);
    let start = ostd::syscall::sys_get_time_ms();
    let mut attempts = 0;
    while poll_pending(start, attempts) {
        attempts += 1;
        let mut buf = [0; IPC_BUF_SIZE];
        match call(
            &NetRequest::UdpRecv {
                cap_id: id,
                buf_len: len,
            },
            &mut buf,
        ) {
            Some(NetResponse::Data(bytes)) if bytes.len() >= 6 => {
                lua.push_ip([bytes[0], bytes[1], bytes[2], bytes[3]]);
                lua.push_count(u16::from_le_bytes([bytes[4], bytes[5]]) as usize);
                lua.push_bytes(&bytes[6..]);
                return 3;
            }
            Some(NetResponse::Data(_)) => ostd::task::yield_now(),
            _ => return lua.push_nil(),
        }
    }
    lua.push_nil()
}

/// `vnet.resolve(name)` -> dotted IPv4 | nil. DNS lives in the net service.
pub unsafe extern "C" fn vnet_resolve(L: *mut LuaState) -> c_int {
    // SAFETY: Lua passes its live state for the duration of this callback.
    let lua = unsafe { LuaStack::new(L) };
    let Some(host) = lua.arg_bytes(1).and_then(|b| core::str::from_utf8(b).ok()) else {
        return lua.push_nil();
    };
    let mut buf = [0; IPC_BUF_SIZE];
    match call(&NetRequest::Resolve { hostname: host }, &mut buf) {
        Some(NetResponse::Addr(ip)) => {
            lua.push_ip(ip);
            1
        }
        _ => lua.push_nil(),
    }
}
