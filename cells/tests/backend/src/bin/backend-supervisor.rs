// SPDX-License-Identifier: MPL-2.0

//! `backend-supervisor` — B0 witness entry point (ADR-0021).
//!
//! `spawn = true` is what grants `SpawnCap`, which in turn is what the kernel
//! requires for `SpawnFromPath`, `NotifyOnExit`, and `ForceExit`: without it the
//! supervisor could neither create nor restart a child.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate ostd;

use ostd::io::println;

api::declare_manifest!(block_io = false, network = false, spawn = true);

api::declare_syscalls![
    Log,
    Exit,
    Send,
    Recv,
    RecvTimeout,
    GetTime,
    Yield,
    // The supervisor reaches its capability-free worker through VFS + SpawnFromElf
    // (`ostd::syscall::sys_spawn_from_path` reads the ELF into a grant first), so it
    // needs service lookup, the grant trio, and the shared outward-launch bit.
    LookupService,
    GrantAlloc,
    GrantShare,
    GrantFree,
    SpawnFromPath,
    NotifyOnExit,
    ForceExit
];

ostd::cell_main!(cell_main);

fn cell_main() {
    println("[backend-supervisor] starting");
    app_backend::run_supervisor()
}
