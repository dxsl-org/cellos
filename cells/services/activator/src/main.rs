//! The Ocel engine activator (Phase 13: the endpoint left init).
//!
//! Trusted init launches `/bin/activator` after the machine-pin gate and
//! registers it as `OCEL_ACTIVATOR`. On an `OCLSREQ1` lease request it starts
//! the fixed demand engine on its own `activator` launch edge (kernel path
//! route, else the VFS ELF route), waits for its readiness, and publishes it as
//! `OCEL_JS` or `OCEL_PDF`: the kernel accepts those two ids from this launch
//! identity only for a live root it launched itself. Leases are bounded and
//! revoked on the owner's death; an unused engine is stopped.

// Test builds only: a hosted `cargo test` gets std and libtest's entry, so the
// lease state machine runs as host unit tests.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

api::declare_manifest!(block_io = false, network = false, spawn = true);
api::declare_syscalls![
    Send,
    SendGather,
    Recv,
    TryRecv,
    RecvTimeout,
    IpcSubmit,
    IpcTake,
    IpcWait,
    IpcCancel,
    IpcCurrent,
    IpcReply,
    Log,
    Heartbeat,
    LookupServiceBound,
    SpawnFromPath,
    SpawnFromElf,
    GetTime,
    GrantAlloc,
    GrantShare,
    GrantFree,
    ForceExit,
    NotifyOnExit,
    RegisterService,
    GetProcs,
];

mod activation;
mod state;

use ostd::service::ServeConfig;

ostd::cell_main!(extern "C" cell_main);

fn cell_main() {
    let mut activator = activation::Activator::new();
    ostd::service::serve(ServeConfig::with_heartbeat(), &mut activator)
}

/// Scheduler ticks (10 ms slices), not the raw architected GetTime counter.
pub(crate) fn now_ticks() -> u64 {
    ostd::syscall::sys_get_scheduler_ticks().unwrap_or(0)
}
