//! POSIX shim integration test cell.
//!
//! Tests the C ABI shims in `libs/api/src/posix.rs`:
//!   - getentropy(2) via sys_get_random (opcode 214)
//!   - socket / connect / send / recv / close via typed Net IPC
//!
//! Spawn with: `posix-shim-test` from the shell.
//! The integration test in tests/integration/tests/boot.rs checks for:
//!   "POSIX-ENTROPY: OK" and "POSIX-NET: OK"

#![no_std]
#![no_main]
extern crate alloc;
extern crate ostd;

use alloc::format;
use core::ffi::c_void;
use core::mem;
use ostd::io::println;

// Network shim test connects to the QEMU SLIRP host echo server.
// Port must match POSIX_SHIM_ECHO_PORT in boot.rs.
const ECHO_IP: [u8; 4] = [10, 0, 2, 2];
const ECHO_PORT: u16 = 10009;

api::declare_manifest!(block_io = false, network = false, spawn = false);
// GetRandom needed for getentropy; Send/Recv/LookupService for net-service IPC.
api::declare_syscalls![Send, Recv, Log, LookupService, GetRandom];

#[no_mangle]
pub fn main() {
    test_getentropy();
    test_net();
}

fn test_getentropy() {
    let mut buf = [0u8; 16];
    // SAFETY: buf is a valid 16-byte stack buffer; posix shim validates len ≤ 256.
    let ret = unsafe {
        api::posix::getentropy(buf.as_mut_ptr() as *mut c_void, 16)
    };
    if ret == 0 && buf.iter().any(|b| *b != 0) {
        println("[posix-shim] POSIX-ENTROPY: OK");
    } else {
        println(&format!("[posix-shim] POSIX-ENTROPY: FAIL ret={ret}"));
    }
}

fn test_net() {
    // AF_INET=2, SOCK_STREAM=1, protocol=0
    let fd = unsafe { api::posix::socket(2, 1, 0) };
    if fd < 0 {
        println("[posix-shim] POSIX-NET: FAIL socket");
        return;
    }

    let addr = api::posix::sockaddr_in {
        sin_family: 2u16,
        sin_port: ECHO_PORT.to_be(),           // port in network byte order
        sin_addr: u32::from_be_bytes(ECHO_IP), // addr in network byte order
        sin_zero: [0u8; 8],
    };
    // SAFETY: addr is a valid sockaddr_in on the stack; addrlen matches.
    let ret = unsafe {
        api::posix::connect(
            fd,
            &addr as *const _ as *const c_void,
            mem::size_of::<api::posix::sockaddr_in>() as i32,
        )
    };
    if ret < 0 {
        println("[posix-shim] POSIX-NET: FAIL connect");
        unsafe { api::posix::_close(fd); }
        return;
    }

    let msg = b"hello\n";
    // SAFETY: msg is valid; len ≤ 495 (well within single-IPC limit).
    let sent = unsafe {
        api::posix::send(fd, msg.as_ptr() as *const c_void, msg.len(), 0)
    };
    if sent < 0 {
        println(&format!("[posix-shim] POSIX-NET: FAIL send sent={sent}"));
        unsafe { api::posix::_close(fd); }
        return;
    }

    let mut rbuf = [0u8; 64];
    // SAFETY: rbuf is valid; len matches capacity.
    let n = unsafe {
        api::posix::recv(fd, rbuf.as_mut_ptr() as *mut c_void, rbuf.len(), 0)
    };
    unsafe { api::posix::_close(fd); }

    if n > 0 {
        println("[posix-shim] POSIX-NET: OK");
    } else {
        println(&format!("[posix-shim] POSIX-NET: FAIL recv n={n}"));
    }
}
