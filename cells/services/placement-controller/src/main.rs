#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use ostd::io::println;

api::declare_manifest!(block_io = false, network = true, spawn = false);
api::declare_syscalls![
    Send,
    Recv,
    TryRecv,
    Log,
    Heartbeat,
    LookupService,
    StateStash,
    StateRestore
];

fn cell_main() {
    println("[placement-controller] Placement Controller initialized (P08-D)");
    loop {
        ostd::syscall::sys_heartbeat(100);
        ostd::task::yield_now();
    }
}

ostd::cell_main!(cell_main);
