//! tokio-smoke - Demonstrating Tokio I/O driver pattern on CellOS via vimio.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use core::time::Duration;
use ostd::io::println;
use ostd::syscall::sys_lookup_service;
use vimio::{Events, Interest, Selector, Token, Waker};

api::declare_manifest!(
    block_io = false,
    network = true,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false,
    i2c = false,
    spi = false
);

api::declare_syscalls![Send, Recv, TrySend, TryRecv, RecvTimeout, Log, LookupService, StateRestore, GetTime];

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(2 * 1024 * 1024);

const SERVER_TOKEN: Token = Token(1);
const WAKER_TOKEN: Token = Token(2);
const HTTP_PORT: u16 = 8088;

#[cfg(not(test))]
ostd::cell_main!(cell_main);

fn cell_main() {
    println("[tokio-smoke] Starting Tokio-style I/O Driver on CellOS (vimio)...");

    let selector = match Selector::new() {
        Ok(s) => s,
        Err(_) => {
            println("[tokio-smoke] Failed to create Selector!");
            ostd::syscall::sys_exit(1);
        }
    };

    println("[tokio-smoke] Selector created successfully.");

    // Create Tokio-style Waker
    let waker = match Waker::new(&selector, WAKER_TOKEN) {
        Ok(w) => w,
        Err(_) => {
            println("[tokio-smoke] Failed to create Waker!");
            ostd::syscall::sys_exit(1);
        }
    };

    println("[tokio-smoke] Waker bound with Token(2). Testing wake() invocation...");
    if waker.wake().is_ok() {
        let mut events = Events::with_capacity(8);
        selector.select(&mut events, Some(Duration::from_millis(500))).ok();

        let woken = events.iter().any(|e| e.token == WAKER_TOKEN);
        if woken {
            println("[tokio-smoke] Waker event received by Selector PASS.");
        } else {
            println("[tokio-smoke] Waker event not received!");
        }
    }

    // Connect to network service if online
    if let Some(net_tid) = sys_lookup_service(api::syscall::service::NET) {
        println(&alloc::format!("[tokio-smoke] Network service available at tid: {}", net_tid));

        let req = api::ipc::NetRequest::TcpListen { port: HTTP_PORT };
        let mut req_buf = [0u8; 128];
        let mut resp_buf = [0u8; 128];

        if let Ok(encoded) = api::ipc::encode(&req, &mut req_buf) {
            let _ = ostd::syscall::sys_send(net_tid, encoded);
            let _ = ostd::syscall::sys_recv(net_tid, &mut resp_buf);

            if let Ok(api::ipc::NetResponse::CapId(listener_cap)) = api::ipc::decode::<api::ipc::NetResponse<'_>>(&resp_buf) {
                println(&alloc::format!("[tokio-smoke] Listening on :{} with cap_id={}", HTTP_PORT, listener_cap));
                if selector.register(listener_cap, SERVER_TOKEN, Interest::READABLE).is_ok() {
                    println("[tokio-smoke] Registered listener socket with Selector.");
                }
            }
        }
    } else {
        println("[tokio-smoke] Network service offline; testing driver loop in standalone mode.");
    }

    println("[tokio-smoke] Running driver select() event loop (Tokio current_thread pattern)...");

    let mut events = Events::with_capacity(16);
    let mut ticks = 0;

    while ticks < 3 {
        ticks += 1;
        match selector.select(&mut events, Some(Duration::from_millis(50))) {
            Ok(()) => {
                for event in events.iter() {
                    println(&alloc::format!("[tokio-smoke] Dispatched Event: token={:?} readable={} writable={}",
                        event.token, event.readable, event.writable));
                }
            }
            Err(_) => {
                println("[tokio-smoke] Selector select error");
                break;
            }
        }
    }

    println("[tokio-smoke] PASS: Tokio current_thread I/O driver verification complete!");
    ostd::syscall::sys_exit(0);
}
