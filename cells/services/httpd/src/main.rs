//! httpd — HTTP/1.1 web server Cell for ViCell.
//!
//! Listens on port 8080. Serves HTML pages (format!-built), static files from VFS,
//! and a JSON REST API. One connection at a time (sufficient for G1 robot LAN use).
//!
//! # Library note
//! Uses httparse for request parsing instead of edge-http: the workspace pins
//! embedded-io-async 0.7 while edge-http 0.7 requires 0.6, and implementing
//! TcpSplit over ViCell's synchronous IPC adds complexity with no G1 benefit.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use api::syscall::service;
use ostd::io::{print, println};
use ostd::syscall::{sys_lookup_service, sys_yield};

mod handlers;
mod net_ipc;
#[cfg(test)]
mod net_ipc_tests;
mod router;
mod static_files;

api::declare_syscalls![Send, Recv, Log, LookupService, StateRestore];

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

    let listen_cap = match net_ipc::tcp_listen(port, net_ep) {
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
    loop {
        // TcpAccept blocks in the kernel until a client connects.
        let stream_cap = loop {
            match net_ipc::tcp_accept(listen_cap, net_ep) {
                Some(c) => break c,
                None => {
                    sys_yield();
                }
            }
        };

        if !router::handle_connection(stream_cap, net_ep, vfs_ep, file_to_serve.as_deref()) {
            println("httpd: response send failed");
        }

        // Yield so smoltcp can flush the TX ring before we send FIN.
        for _ in 0..200 {
            sys_yield();
        }
        net_ipc::tcp_close(stream_cap, net_ep);
    }
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
