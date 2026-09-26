//! POSIX shim integration test cell.
//!
//! Tests the C ABI shims in `libs/api/src/services/posix.rs`:
//!   - open/fstat/close with truthful, failure-atomic descriptor metadata
//!   - stat and unlink with truthful return values and failure atomicity
//!   - mkdir and rmdir on /srv with truthful directory lifecycle results
//!   - rename on /srv with truthful return values and failure atomicity
//!   - getentropy(2) via sys_get_random (opcode 214)
//!   - socket / connect / send / recv / close via typed Net IPC
//!   - gethostbyname via the net service's resolver (literal/alias/DNS)
//!
//! Spawn with: `posix-shim-test` from the shell. Integration tests require the
//! dedicated `POSIX-FSTAT-*`, `POSIX-STAT`, `POSIX-UNLINK`, `POSIX-MKDIR-RMDIR`,
//! `POSIX-RENAME`, `POSIX-ENTROPY`, `POSIX-NET`, and `POSIX-DNS` markers.

#![no_std]
#![no_main]
extern crate alloc;
extern crate ostd;

mod fstat;

use alloc::format;
use core::ffi::{c_char, c_void};
use ostd::io::println;

// Network shim test connects to the QEMU SLIRP host echo server.
// Port must match POSIX_SHIM_ECHO_PORT in boot.rs.
const ECHO_IP: [u8; 4] = [10, 0, 2, 2];
const ECHO_PORT: u16 = 10009;

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_FFI
);
// Add only the file trio needed by the fstat smoke to the existing restrictive set.
api::declare_syscalls![
    Send,
    Recv,
    Log,
    LookupService,
    GetTime,
    // The entropy smoke calls `getentropy`, which the shim implements over
    // `GetRandom` (opcode 214). Without the allowlist bit the call is denied and
    // the smoke can only ever report `POSIX-ENTROPY: FAIL ret=-1`.
    GetRandom,
    Open,
    Fstat,
    Rename,
    Close,
    VfsMutate
];

// Declare C ABI directly — works whether the symbols come from api::posix (Tier A)
// or mlibc/libc.a (Tier B); avoids Rust feature-unification breaking the lookup.
#[repr(C)]
struct SockaddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: u32,
    sin_zero: [u8; 8],
}

extern "C" {
    fn getentropy(buf: *mut c_void, buflen: usize) -> i32;
    fn socket(domain: i32, typ: i32, protocol: i32) -> i32;
    fn connect(fd: i32, addr: *const c_void, addrlen: i32) -> i32;
    fn send(fd: i32, buf: *const c_void, len: usize, flags: i32) -> isize;
    fn recv(fd: i32, buf: *mut c_void, len: usize, flags: i32) -> isize;
    #[link_name = "_close"]
    fn close(fd: i32) -> i32;
    fn gethostbyname(name: *const c_char) -> *mut Hostent;
}

/// `struct hostent` in the mlibc `netdb.h` layout.
#[repr(C)]
struct Hostent {
    h_name: *mut c_char,
    h_aliases: *mut *mut c_char,
    h_addrtype: i32,
    h_length: i32,
    h_addr_list: *mut *mut c_char,
}

extern "C" {
    fn cellos_porting_smoke() -> i32;
}

/// Platform-host hook consumed by the external CMake/Meson smoke source.
#[no_mangle]
pub extern "C" fn cellos_time_ms() -> u64 {
    port_platform::PlatformHost::new().time_ms()
}

#[no_mangle]
pub fn main() {
    fstat::test_fstat();
    fstat::test_stat();
    fstat::test_unlink();
    fstat::test_mkdir_rmdir();
    fstat::test_raw_rename();
    fstat::test_rename();
    test_getentropy();
    test_net();
    test_net_dns();
    test_porting_smoke();
}

fn test_porting_smoke() {
    // SAFETY: the external static C archive is linked by build.rs and has no
    // preconditions beyond the Rust host callback above.
    if unsafe { cellos_porting_smoke() } == 0 {
        println("[posix-shim] PORTING-SMOKE: OK");
    } else {
        println("[posix-shim] PORTING-SMOKE: FAIL");
    }
}

fn test_getentropy() {
    let mut buf = [0u8; 16];
    // SAFETY: buf is a valid 16-byte stack buffer; shim validates len ≤ 256.
    let ret = unsafe { getentropy(buf.as_mut_ptr() as *mut c_void, 16) };
    if ret == 0 && buf.iter().any(|b| *b != 0) {
        println("[posix-shim] POSIX-ENTROPY: OK");
    } else {
        println(&format!("[posix-shim] POSIX-ENTROPY: FAIL ret={ret}"));
    }
}

fn test_net() {
    match echo_round_trip(ECHO_IP) {
        Ok(_) => println("[posix-shim] POSIX-NET: OK"),
        Err(stage) => println(&format!("[posix-shim] POSIX-NET: FAIL {stage}")),
    }
}

/// Name resolution through the C ABI: `gethostbyname` → the net service's
/// resolver → a usable address.
///
/// `gateway` is SLIRP's name for the host loopback (10.0.2.2, the same host the
/// literal leg above uses), which the service answers from its alias table — so
/// this witness needs no upstream DNS server and stays deterministic.
fn test_net_dns() {
    let name = b"gateway\0";
    // SAFETY: `name` is NUL-terminated; the shim returns a pointer to its own
    // static hostent (or null).
    let ent = unsafe { gethostbyname(name.as_ptr() as *const c_char) };
    if ent.is_null() {
        println("[posix-shim] POSIX-DNS: FAIL gethostbyname returned NULL");
        return;
    }

    // SAFETY: `ent` is the shim's static hostent, filled by the call above; a
    // successful lookup always publishes a one-element h_addr_list.
    let ip = unsafe {
        let list = (*ent).h_addr_list;
        if (*ent).h_addrtype != 2 || (*ent).h_length != 4 || list.is_null() || (*list).is_null() {
            println(&format!(
                "[posix-shim] POSIX-DNS: FAIL family={} len={}",
                (*ent).h_addrtype,
                (*ent).h_length
            ));
            return;
        }
        let mut ip = [0u8; 4];
        core::ptr::copy_nonoverlapping(*list as *const u8, ip.as_mut_ptr(), 4);
        ip
    };

    if ip != ECHO_IP {
        println(&format!("[posix-shim] POSIX-DNS: FAIL addr={ip:?}"));
        return;
    }

    // The address must be usable, not merely present: connect through it.
    match echo_round_trip(ip) {
        Ok(_) => println("[posix-shim] POSIX-DNS: OK"),
        Err(stage) => println(&format!("[posix-shim] POSIX-DNS: FAIL {stage}")),
    }
}

/// One echo round trip over the socket shim: connect → send → recv → close.
///
/// `Err` names the stage that failed, with the shim's return value, so the
/// caller can report exactly where the C ABI stopped.
fn echo_round_trip(ip: [u8; 4]) -> Result<isize, alloc::string::String> {
    // AF_INET=2, SOCK_STREAM=1, protocol=0
    let fd = unsafe { socket(2, 1, 0) };
    if fd < 0 {
        return Err(format!("socket fd={fd}"));
    }

    let addr = SockaddrIn {
        sin_family: 2u16,
        sin_port: ECHO_PORT.to_be(),
        sin_addr: u32::from_be_bytes(ip),
        sin_zero: [0u8; 8],
    };
    // SAFETY: addr is a valid SockaddrIn on the stack; addrlen matches.
    let ret = unsafe {
        connect(
            fd,
            &addr as *const _ as *const c_void,
            core::mem::size_of::<SockaddrIn>() as i32,
        )
    };
    if ret < 0 {
        unsafe {
            close(fd);
        }
        return Err(format!("connect ret={ret}"));
    }

    let msg = b"hello\n";
    let sent = unsafe { send(fd, msg.as_ptr() as *const c_void, msg.len(), 0) };
    if sent < 0 {
        unsafe {
            close(fd);
        }
        return Err(format!("send ret={sent}"));
    }
    let mut rbuf = [0u8; 64];
    let mut n: isize = -1;
    for _ in 0..2000 {
        n = unsafe { recv(fd, rbuf.as_mut_ptr() as *mut c_void, rbuf.len(), 0) };
        if n > 0 {
            break;
        }
        ostd::syscall::sys_yield();
    }
    unsafe {
        close(fd);
    }

    if n > 0 {
        Ok(n)
    } else {
        Err(format!("recv n={n}"))
    }
}
