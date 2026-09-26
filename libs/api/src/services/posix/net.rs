// SPDX-License-Identifier: MPL-2.0
// BSD socket shims: socket / connect / send / recv / close
//
// Socket fd range: 10–17 (above stdio 0–2 and shell-reserved 3–9).
// The fd→cap_id mapping is stored in SOCK_CAPS[] (one slot per fd).
//
// Thread safety: ViCell is single-hart for G1; AtomicU32 CAS prevents
// double-alloc from interrupt context.

#![allow(unsafe_code)]

use super::sysio::raw_syscall;
use crate::ipc::{decode, encode, NetRequest, NetResponse, IPC_BUF_SIZE};
use crate::syscall::ViSyscall;
use core::cell::UnsafeCell;
use core::ffi::{c_char, c_int, c_void};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub(super) const SOCK_BASE_FD: c_int = 10;
const MAX_SOCKETS: usize = 8;

const AF_INET: c_int = 2;
const SOCK_STREAM: c_int = 1;

/// Ceiling on one blocking `send` while the socket is still connecting.
const SEND_BUDGET_MS: u64 = 5_000;
/// Backstop when the cell cannot read `GetTime`: two IPC round-trips plus a
/// yield per attempt, so this is a wall-clock bound in practice too.
const SEND_ATTEMPT_CEILING: usize = 100_000;

static NET_TID_CACHE: AtomicUsize = AtomicUsize::new(0);

/// cap_id slot per socket fd. 0 = free, u32::MAX = reserved (alloc in progress).
static SOCK_CAPS: [AtomicU32; MAX_SOCKETS] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
];

/// Internet socket address (mirrors `struct sockaddr_in`).
#[repr(C)]
pub struct sockaddr_in {
    pub sin_family: u16,
    pub sin_port: u16,
    pub sin_addr: u32,
    pub sin_zero: [u8; 8],
}

fn net_tid() -> usize {
    let cached = NET_TID_CACHE.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    // LookupService = 206, service::NET = 2
    let tid = unsafe { raw_syscall(ViSyscall::LookupService, 2, 0, 0, 0) };
    if tid > 0 {
        NET_TID_CACHE.store(tid as usize, Ordering::Relaxed);
        tid as usize
    } else {
        0
    }
}

fn alloc_fd() -> Option<(c_int, usize)> {
    for (i, cap) in SOCK_CAPS.iter().enumerate() {
        if cap
            .compare_exchange(0, u32::MAX, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return Some((SOCK_BASE_FD + i as c_int, i));
        }
    }
    None
}

fn cap_from_fd(fd: c_int) -> Option<u32> {
    let idx = fd - SOCK_BASE_FD;
    if idx < 0 || idx as usize >= MAX_SOCKETS {
        return None;
    }
    let cap = SOCK_CAPS[idx as usize].load(Ordering::Acquire);
    if cap == 0 || cap == u32::MAX {
        None
    } else {
        Some(cap)
    }
}

/// Allocate a socket fd (AF_INET/SOCK_STREAM only).
///
/// # Safety
/// No pointer arguments; safe to call with any integer values. Caller must
/// still route the returned fd through the other functions in this module
/// (it is not a kernel fd and is meaningless to raw syscalls).
#[no_mangle]
pub unsafe extern "C" fn socket(domain: c_int, type_: c_int, _protocol: c_int) -> c_int {
    if domain != AF_INET || type_ != SOCK_STREAM {
        return -1;
    }
    match alloc_fd() {
        Some((fd, _)) => fd,
        None => -1,
    }
}

/// # Safety
/// `addr` must be either null or point to a readable, initialized
/// `sockaddr_in` of at least `addrlen` bytes for the duration of the call;
/// the caller retains ownership and this function does not read past
/// `size_of::<sockaddr_in>()` bytes.
#[no_mangle]
pub unsafe extern "C" fn connect(fd: c_int, addr: *const c_void, addrlen: c_int) -> c_int {
    let idx = fd - SOCK_BASE_FD;
    if idx < 0 || idx as usize >= MAX_SOCKETS {
        return -1;
    }
    if addr.is_null() || addrlen < core::mem::size_of::<sockaddr_in>() as c_int {
        return -1;
    }
    let net = net_tid();
    if net == 0 {
        return -1;
    }

    let sin = addr as *const sockaddr_in;
    if (*sin).sin_family != AF_INET as u16 {
        return -1;
    }
    let ip = (*sin).sin_addr.to_be_bytes();
    let port = u16::from_be((*sin).sin_port);

    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let req = NetRequest::TcpConnect { addr: ip, port };
    let Ok(encoded) = encode(&req, &mut req_buf) else {
        return -1;
    };
    raw_syscall(
        ViSyscall::Send,
        net,
        encoded.as_ptr() as usize,
        encoded.len(),
        0,
    );

    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let n = raw_syscall(
        ViSyscall::Recv,
        0,
        resp_buf.as_mut_ptr() as usize,
        resp_buf.len(),
        0,
    );
    if n <= 0 {
        return -1;
    }

    match decode::<NetResponse>(&resp_buf[..n as usize]) {
        Ok(NetResponse::CapId(cap)) if cap > 0 => {
            SOCK_CAPS[idx as usize].store(cap, Ordering::Release);
            0
        }
        _ => -1,
    }
}

/// smoltcp `State::Closed` as `NetRequest::SocketState` reports it.
const TCP_STATE_CLOSED: u8 = 0x00;

/// One `SocketState` round-trip; `None` when the service cannot answer.
fn socket_state(cap: u32, net: usize) -> Option<u8> {
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let req = NetRequest::SocketState { cap_id: cap };
    let encoded = encode(&req, &mut req_buf).ok()?;
    // SAFETY: `encoded` is a live, initialized buffer and `net` is a task id
    // returned by LookupService; the syscall only reads those bytes.
    unsafe {
        raw_syscall(
            ViSyscall::Send,
            net,
            encoded.as_ptr() as usize,
            encoded.len(),
            0,
        );
    }
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    // SAFETY: `resp_buf` is a live, writable buffer; the kernel writes at most
    // `resp_buf.len()` bytes and reports how many.
    let n = unsafe {
        raw_syscall(
            ViSyscall::Recv,
            0,
            resp_buf.as_mut_ptr() as usize,
            resp_buf.len(),
            0,
        )
    };
    if n <= 0 {
        return None;
    }
    match decode::<NetResponse>(&resp_buf[..n as usize]) {
        Ok(NetResponse::State(state)) => Some(state),
        _ => None,
    }
}

/// Send up to 495 bytes per call (IPC payload ceiling after postcard framing).
///
/// # Safety
/// `buf` must be either null or point to at least `len` readable, initialized
/// bytes for the duration of the call (only up to 495 of them are actually read).
#[no_mangle]
pub unsafe extern "C" fn send(fd: c_int, buf: *const c_void, len: usize, _flags: c_int) -> c_int {
    if buf.is_null() {
        return -1;
    }
    let Some(cap) = cap_from_fd(fd) else {
        return -1;
    };
    let net = net_tid();
    if net == 0 {
        return -1;
    }

    let capped = len.min(495);
    let data = core::slice::from_raw_parts(buf as *const u8, capped);
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let req = NetRequest::TcpSend { cap_id: cap, data };
    let Ok(encoded) = encode(&req, &mut req_buf) else {
        return -1;
    };

    // smoltcp accepts nothing until the socket reaches Established, so a
    // `send()` straight after `connect()` waits on the handshake — a wait the
    // network owns, not the scheduler. Bound it with the wall clock (POSIX
    // blocking-send semantics) and keep the iteration count as the backstop for
    // a cell that cannot read `GetTime`.
    let start_ms = raw_syscall(ViSyscall::GetTime, 1, 0, 0, 0);
    let clock_available = start_ms >= 0;

    for _attempt in 0..SEND_ATTEMPT_CEILING {
        raw_syscall(
            ViSyscall::Send,
            net,
            encoded.as_ptr() as usize,
            encoded.len(),
            0,
        );
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        let n = raw_syscall(
            ViSyscall::Recv,
            0,
            resp_buf.as_mut_ptr() as usize,
            resp_buf.len(),
            0,
        );
        if n <= 0 {
            return -1;
        }
        match decode::<NetResponse>(&resp_buf[..n as usize]) {
            Ok(NetResponse::Data(bytes)) if bytes.len() >= 4 => {
                let accepted = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                if accepted > 0 || capped == 0 {
                    return (accepted as i32).min(capped as i32);
                }
                // A socket the peer refused (RST lands in Closed) never becomes
                // writable again: report the failure now instead of burning the
                // handshake budget, which is what a caller without a listener
                // would otherwise pay for a full 5 s per call.
                if socket_state(cap, net) == Some(TCP_STATE_CLOSED) {
                    return -1;
                }
                if clock_available {
                    let now_ms = raw_syscall(ViSyscall::GetTime, 1, 0, 0, 0);
                    if now_ms >= 0
                        && (now_ms as u64).saturating_sub(start_ms as u64) > SEND_BUDGET_MS
                    {
                        return -1;
                    }
                }
                raw_syscall(ViSyscall::Yield, 0, 0, 0, 0);
            }
            _ => return -1,
        }
    }
    -1
}

/// # Safety
/// `buf` must be either null or point to at least `len` writable bytes for
/// the duration of the call; only up to the number of bytes actually
/// received (never more than `len`) are written.
#[no_mangle]
pub unsafe extern "C" fn recv(fd: c_int, buf: *mut c_void, len: usize, _flags: c_int) -> c_int {
    if buf.is_null() {
        return -1;
    }
    let Some(cap) = cap_from_fd(fd) else {
        return -1;
    };
    let net = net_tid();
    if net == 0 {
        return -1;
    }

    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let req = NetRequest::TcpRecv {
        cap_id: cap,
        buf_len: len as u32,
    };
    let Ok(encoded) = encode(&req, &mut req_buf) else {
        return -1;
    };
    raw_syscall(
        ViSyscall::Send,
        net,
        encoded.as_ptr() as usize,
        encoded.len(),
        0,
    );

    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let n = raw_syscall(
        ViSyscall::Recv,
        0,
        resp_buf.as_mut_ptr() as usize,
        resp_buf.len(),
        0,
    );
    if n <= 0 {
        return 0;
    }

    match decode::<NetResponse>(&resp_buf[..n as usize]) {
        Ok(NetResponse::Data(data)) => {
            let copy_len = data.len().min(len);
            core::ptr::copy_nonoverlapping(data.as_ptr(), buf as *mut u8, copy_len);
            copy_len as c_int
        }
        _ => -1,
    }
}

// _close dispatches socket fds here; regular fds go to the kernel Close syscall.
///
/// # Safety
/// No pointer arguments. `handle` must be an fd previously returned by
/// `socket()` or another kernel-fd-returning call; passing an arbitrary
/// integer is safe (returns an error) but closing an fd still in use by
/// another thread races with that use, per standard POSIX close() semantics.
#[no_mangle]
pub unsafe extern "C" fn _close(handle: c_int) -> c_int {
    if handle >= SOCK_BASE_FD && handle < SOCK_BASE_FD + MAX_SOCKETS as c_int {
        return socket_close(handle);
    }
    raw_syscall(ViSyscall::Close, handle as usize, 0, 0, 0) as c_int
}

/// # Safety
/// Standard POSIX close() semantics.
#[no_mangle]
pub unsafe extern "C" fn close(handle: c_int) -> c_int {
    _close(handle)
}

unsafe fn socket_close(fd: c_int) -> c_int {
    let idx = (fd - SOCK_BASE_FD) as usize;
    let cap = SOCK_CAPS[idx].load(Ordering::Acquire);
    if cap == 0 {
        return -1;
    }

    if cap != u32::MAX {
        let net = net_tid();
        if net != 0 {
            let mut req_buf = [0u8; IPC_BUF_SIZE];
            let req = NetRequest::TcpClose { cap_id: cap };
            if let Ok(encoded) = encode(&req, &mut req_buf) {
                raw_syscall(
                    ViSyscall::Send,
                    net,
                    encoded.as_ptr() as usize,
                    encoded.len(),
                    0,
                );
                let mut r = [0u8; 4];
                raw_syscall(ViSyscall::Recv, 0, r.as_mut_ptr() as usize, r.len(), 0);
            }
        }
    }
    SOCK_CAPS[idx].store(0, Ordering::Release);
    0
}

// ── Name resolution ──────────────────────────────────────────────────────────

/// `struct hostent` in the mlibc `netdb.h` layout.
#[repr(C)]
pub struct hostent {
    pub h_name: *mut c_char,
    pub h_aliases: *mut *mut c_char,
    pub h_addrtype: c_int,
    pub h_length: c_int,
    pub h_addr_list: *mut *mut c_char,
}

/// Static storage for the one result `gethostbyname` may hand out.
///
/// POSIX defines the return as static storage that the next lookup overwrites —
/// no allocation, nothing for the caller to free. Single-hart shim, so the
/// result is deliberately not thread-safe, exactly like mlibc's own
/// implementation under the same contract.
struct HostResult {
    ent: UnsafeCell<hostent>,
    name: UnsafeCell<[u8; 64]>,
    addr: UnsafeCell<[u8; 4]>,
    aliases: UnsafeCell<[*mut c_char; 1]>,
    addr_list: UnsafeCell<[*mut c_char; 2]>,
}

// SAFETY: the buffers are only written and read inside `gethostbyname`, which
// runs on the cell's single thread; the shared pointer is the POSIX-mandated
// static result.
unsafe impl Sync for HostResult {}

static HOST_RESULT: HostResult = HostResult {
    ent: UnsafeCell::new(hostent {
        h_name: core::ptr::null_mut(),
        h_aliases: core::ptr::null_mut(),
        h_addrtype: 0,
        h_length: 0,
        h_addr_list: core::ptr::null_mut(),
    }),
    name: UnsafeCell::new([0u8; 64]),
    addr: UnsafeCell::new([0u8; 4]),
    aliases: UnsafeCell::new([core::ptr::null_mut()]),
    addr_list: UnsafeCell::new([core::ptr::null_mut(); 2]),
};

/// Longest name the shim will scan for a terminator.
const MAX_NAME_SCAN: usize = 255;

/// Resolve `name` through the net service: the service owns the whole order
/// (IPv4 literal, SLIRP alias, UDP A-record query to the DHCP-leased server),
/// so the C surface carries no resolver of its own.
fn resolve_name(name: &str) -> Option<[u8; 4]> {
    let net = net_tid();
    if net == 0 {
        return None;
    }
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let req = NetRequest::Resolve { hostname: name };
    let Ok(encoded) = encode(&req, &mut req_buf) else {
        return None;
    };
    // SAFETY: `encoded` is a live, initialized buffer and `net` is a task id
    // returned by LookupService; the syscall only reads those bytes.
    unsafe {
        raw_syscall(
            ViSyscall::Send,
            net,
            encoded.as_ptr() as usize,
            encoded.len(),
            0,
        );
    }

    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    // SAFETY: `resp_buf` is a live, writable buffer; the kernel writes at most
    // `resp_buf.len()` bytes and reports how many.
    let n = unsafe {
        raw_syscall(
            ViSyscall::Recv,
            0,
            resp_buf.as_mut_ptr() as usize,
            resp_buf.len(),
            0,
        )
    };
    if n <= 0 {
        return None;
    }
    match decode::<NetResponse>(&resp_buf[..n as usize]) {
        Ok(NetResponse::Addr(addr)) => Some(addr),
        _ => None,
    }
}

/// Resolve a hostname to an IPv4 address (`struct hostent` form).
///
/// Returns `NULL` for a null/empty/unterminated name, an unresolvable name, or
/// an unreachable net service. `h_name` echoes the queried name, `h_aliases` is
/// empty, and `h_addr_list` holds exactly one address followed by `NULL`.
///
/// # Safety
/// `name` must be null or point to a NUL-terminated string whose terminator is
/// within 255 bytes; the returned pointer aliases static storage that the next
/// call in this module overwrites.
#[no_mangle]
pub unsafe extern "C" fn gethostbyname(name: *const c_char) -> *mut hostent {
    if name.is_null() {
        return core::ptr::null_mut();
    }
    let mut len = 0usize;
    // Bounded scan: a missing terminator must not walk off the mapping.
    while len < MAX_NAME_SCAN && *name.add(len) != 0 {
        len += 1;
    }
    if len == 0 || len == MAX_NAME_SCAN {
        return core::ptr::null_mut();
    }
    let bytes = core::slice::from_raw_parts(name.cast::<u8>(), len);
    let Ok(text) = core::str::from_utf8(bytes) else {
        return core::ptr::null_mut();
    };
    // Everything goes to the service, literals included: resolution policy
    // lives there, and a second parser here would only drift from it.
    let Some(ip) = resolve_name(text) else {
        return core::ptr::null_mut();
    };

    // SAFETY: single-threaded shim; each buffer is a distinct cell.
    unsafe {
        let name_buf = &mut *HOST_RESULT.name.get();
        let copy = len.min(name_buf.len() - 1);
        name_buf[..copy].copy_from_slice(&bytes[..copy]);
        name_buf[copy] = 0;

        let addr = &mut *HOST_RESULT.addr.get();
        *addr = ip;

        let aliases = &mut *HOST_RESULT.aliases.get();
        aliases[0] = core::ptr::null_mut();

        let addr_list = &mut *HOST_RESULT.addr_list.get();
        addr_list[0] = addr.as_mut_ptr() as *mut c_char;
        addr_list[1] = core::ptr::null_mut();

        let ent = &mut *HOST_RESULT.ent.get();
        ent.h_name = name_buf.as_mut_ptr() as *mut c_char;
        ent.h_aliases = aliases.as_mut_ptr();
        ent.h_addrtype = AF_INET;
        ent.h_length = 4;
        ent.h_addr_list = addr_list.as_mut_ptr();
        ent as *mut hostent
    }
}
