#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

extern crate alloc;
extern crate ostd;

use ostd::io::println;

abi::declare_manifest!(block_io = false, network = false, spawn = false);
abi::declare_syscalls![
    Send,
    Recv,
    TryRecv,
    Log,
    Heartbeat,
    LookupServiceBound,
    StateStash,
    StateRestore
];

fn cell_main() {
    println("[budget-authority] Budget Authority initialized (P08-C)");
    loop {
        ostd::syscall::sys_heartbeat(100);
        ostd::task::yield_now();
    }
}

ostd::cell_main!(cell_main);
