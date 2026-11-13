//! The ADR-0029 machine-pin Cell (Phase 13: the endpoint left init).
//!
//! Trusted init launches `/bin/machine-pin` right after the VFS on x86 and
//! registers it as `MACHINE_PIN`; the kernel accepts that registration only for
//! init's own child signed as `machine-pin`, and answers the machine facts
//! (433) only to that live provider. The Cell runs the gate once, then serves
//! `MachinePinRequest` frames: init asks `Gate` as a client and withholds the
//! services itself; the console shell asks `Status` and `Confirm`.

// Test builds only: a hosted `cargo test` gets std and libtest's entry, so the
// gate's decision table, the diff and the store's absence rule run on the host.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

api::declare_manifest!(block_io = false, network = false, spawn = false);
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
    GetTime,
    GetProcs,
    GrantAlloc,
    GrantShare,
    GrantFree,
    StateRestore,
    // The pin store write is an ordinary VFS mutation: the VFS has no
    // exception for this Cell, so it declares W like any writer.
    VfsMutate,
];

mod diff;
mod endpoint;
mod gate;
mod store;
#[cfg(test)]
mod tests;

use api::syscall::ProcessInfo;
use ostd::service::ServeConfig;

ostd::cell_main!(extern "C" cell_main);

fn cell_main() {
    let init_tid = endpoint::init_tid(&ostd::args::args());
    let (gate, safe_mode) = gate::run(&mut store::VfsPinStore::new());
    let mut endpoint = endpoint::Endpoint::new(gate, safe_mode, init_tid);
    ostd::service::serve(ServeConfig::with_heartbeat(), &mut endpoint)
}

/// Scheduler ticks (10 ms slices).
pub(crate) fn now_ticks() -> u64 {
    ostd::syscall::sys_get_scheduler_ticks().unwrap_or(0)
}

/// Whether `tid` is live. Absence from a truncated process table is not proof
/// of death; larger tables grow only for this query, with a hard bound and
/// fail-closed uncertainty.
pub(crate) fn task_alive(tid: usize) -> Option<bool> {
    let mut stack = [ProcessInfo::default(); 64];
    let count = ostd::syscall::sys_get_procs(&mut stack).ok()?;
    if let Some(row) = stack.iter().take(count).find(|row| row.id == tid) {
        return Some(row.state != 3);
    }
    if count < stack.len() {
        return Some(false);
    }
    let mut rows = alloc::vec![ProcessInfo::default(); 128];
    loop {
        let count = ostd::syscall::sys_get_procs(&mut rows).ok()?;
        if let Some(row) = rows.iter().take(count).find(|row| row.id == tid) {
            return Some(row.state != 3);
        }
        if count < rows.len() {
            return Some(false);
        }
        if rows.len() >= 8192 {
            return None;
        }
        rows.resize(rows.len() * 2, ProcessInfo::default());
    }
}
