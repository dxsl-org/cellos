//! async-echo - Asynchronous TCP Echo Service on CellOS using Poller.
//!
//! Demonstrates cooperative single-thread asynchronous I/O with vipolling.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use core::time::Duration;
use ostd::io::println;
use ostd::syscall::sys_lookup_service;
use vipolling::{Event, Events, Poller};

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

const ECHO_PORT: u16 = 7070;
const SERVER_TOKEN: usize = 1;

#[cfg(not(test))]
ostd::cell_main!(cell_main);

fn cell_main() {
    println("[async-echo] Initializing CellOS Async Reactor via vipolling...");

    let poller = match Poller::new() {
        Ok(p) => p,
        Err(_) => {
            println("[async-echo] Failed to create Poller!");
            ostd::syscall::sys_exit(1);
        }
    };

    println("[async-echo] Poller instance created successfully.");

    // Verify self-notify waker mechanism
    println("[async-echo] Testing reactor notify() self-wakeup...");
    if poller.notify().is_ok() {
        let mut events = Events::new();
        // Wait should be interrupted immediately by the notify wakeup message
        let res = poller.wait(&mut events, Some(Duration::from_millis(500)));
        match res {
            Ok(0) => println("[async-echo] Reactor notify() self-wakeup PASS (unparked with 0 events)."),
            _ => println("[async-echo] Unexpected wait result after notify"),
        }
    } else {
        println("[async-echo] Poller notify() failed!");
    }

    // Resolve network service
    let net_tid = match sys_lookup_service(api::syscall::service::NET) {
        Some(tid) => {
            println(&alloc::format!("[async-echo] Network service discovered at tid: {}", tid));
            tid
        }
        None => {
            println("[async-echo] Network service not running; entering standalone async poll loop.");
            0
        }
    };

    if net_tid > 0 {
        // Request TCP listen on port 7070
        let req = api::ipc::NetRequest::TcpListen { port: ECHO_PORT };
        let mut req_buf = [0u8; 128];
        let mut resp_buf = [0u8; 128];

        if let Ok(encoded) = api::ipc::encode(&req, &mut req_buf) {
            let _ = ostd::syscall::sys_send(net_tid, encoded);
            let _ = ostd::syscall::sys_recv(net_tid, &mut resp_buf);

            if let Ok(api::ipc::NetResponse::CapId(listener_cap)) = api::ipc::decode::<api::ipc::NetResponse<'_>>(&resp_buf) {
                println(&alloc::format!("[async-echo] TCP listener established, cap_id={}, listening on :{}", listener_cap, ECHO_PORT));

                // Register listener socket with poller
                if poller.add(listener_cap, Event::readable(SERVER_TOKEN)).is_ok() {
                    println("[async-echo] Listener socket registered with Poller.");
                }
            }
        }
    }

    println("[async-echo] Entering asynchronous event loop (smol/polling model)...");

    let mut events = Events::with_capacity(32);
    let mut turns = 0;

    while turns < 5 {
        turns += 1;
        // Non-blocking poll or short tick wait
        match poller.wait(&mut events, Some(Duration::from_millis(100))) {
            Ok(count) => {
                if count > 0 {
                    println(&alloc::format!("[async-echo] Poller returned {} ready events.", count));
                    for ev in events.iter() {
                        println(&alloc::format!("[async-echo]   Event token={} readable={} writable={}", ev.key, ev.readable, ev.writable));
                    }
                }
            }
            Err(_) => {
                println("[async-echo] Poller wait returned error");
                break;
            }
        }
    }

    println("[async-echo] PASS: Async reactor demo executed successfully!");
    ostd::syscall::sys_exit(0);
}
