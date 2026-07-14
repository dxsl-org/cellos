//! httpd — HTTP/1.1 web server Cell for ViCell.
//!
//! Listens on port 8080. Serves HTML pages (format!-built), static files from VFS,
//! and a JSON REST API. One connection at a time (sufficient for G1 robot LAN use).
//!
//! # Library note
//! Uses httparse for request parsing instead of edge-http: the workspace pins
//! embedded-io-async 0.7 while edge-http 0.7 requires 0.6, and implementing
//! TcpSplit over ViCell's synchronous IPC adds complexity with no G1 benefit.

#![no_std]
#![no_main]

extern crate alloc;

use api::syscall::service;
use ostd::io::println;
use ostd::syscall::{sys_lookup_service, sys_yield};

mod handlers;
mod net_ipc;
mod router;

api::declare_syscalls![Send, Recv, Log, LookupService, StateRestore];

const HTTPD_PORT: u16 = 8080;

#[no_mangle]
pub fn main() {
    println("httpd: starting");

    let net_ep = wait_for_service(service::NET, "net");
    let vfs_ep = wait_for_service(service::VFS, "vfs");

    let listen_cap = match net_ipc::tcp_listen(HTTPD_PORT, net_ep) {
        Some(c) => c,
        None => {
            println("httpd: TcpListen failed");
            return;
        }
    };

    println("httpd: listening on :8080");

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

        router::handle_connection(stream_cap, net_ep, vfs_ep);

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
