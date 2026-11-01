//! Tier-2 echo peer for `bench ipc_send_recv_domain`.
//!
//! The same echo loop as the SAS `bench-probe ipc-echo` peer, but declared
//! `PROTECTION_CLASS_UNTRUSTED`, so the kernel admits it to a private-root
//! domain and every request/reply crosses the Tier-2 copy path. `bench` spawns it
//! through the Pinned route (`("bench", "/bin/bench-peer-t2")` launch edge).

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

mod echo;

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_UNTRUSTED
);
api::declare_syscalls![Send, Recv, Log, Exit, Yield];

ostd::cell_main!(cell_main);

fn cell_main() {
    echo::run()
}
