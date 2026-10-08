//! httpd — cooperative single-cell HTTP/1.1 server for ViCell.
//! One owner manages up to 257 sockets, including 256 concurrent active clients.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]
// The ostd custom heap macro contains its own audited unsafe initialization.

extern crate alloc;

use alloc::string::String;
use api::syscall::service;
use ostd::io::{print, println};
use ostd::syscall::{sys_lookup_service, sys_yield};

mod handlers;
mod net_ipc;
mod reactor;
mod router;

api::declare_syscalls![Send, Recv, Log, LookupService, StateRestore, GetTime];

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(8 * 1024 * 1024);

const HTTPD_PORT: u16 = 8080;

#[cfg(not(test))]
ostd::cell_main!(cell_main);
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

fn cell_main() {
    #[cfg(target_os = "none")]
    init_custom_heap();
    let argv = ostd::args();
    let mut port = HTTPD_PORT;
    let mut file_to_serve: Option<String> = None;

    if let Some(first) = argv.first() {
        if let Some(p) = parse_u16(first).filter(|&p| p > 0) {
            port = p;
            if argv.len() >= 2 {
                file_to_serve = Some(argv[1].clone());
            }
        } else {
            file_to_serve = Some(first.clone());
        }
    }

    println("httpd: starting");

    let net_ep = wait_for_service(service::NET, "net");
    let vfs_ep = wait_for_service(service::VFS, "vfs");

    let listen_cap = match reactor::listen(net_ep, port) {
        Some(c) => c,
        None => {
            println("httpd: TcpListen failed");
            return;
        }
    };

    if port == HTTPD_PORT && file_to_serve.is_none() {
        println("httpd: listening on :8080");
    } else {
        print("httpd: listening on :");
        ostd::io::print_usize(port as usize);
        println("");
    }
    reactor::run(listen_cap, net_ep, vfs_ep, file_to_serve.as_deref());
}

/// Resolve a well-known service TID, retrying up to 100 times with yield.
/// Panics (via unreachable) only if the service is permanently absent at boot.
fn wait_for_service(id: u16, name: &str) -> usize {
    for _ in 0..100 {
        if let Some(tid) = sys_lookup_service(id) {
            return tid;
        }
        sys_yield();
    }
    // Service absent — print and park rather than panic! (no process death in SAS)
    let _ = name;
    println("httpd: required service not found, parking");
    loop {
        sys_yield();
    }
}
