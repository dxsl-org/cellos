// SPDX-License-Identifier: MPL-2.0

//! `backend-worker` — the supervised child in the B0 witness (ADR-0021).
//!
//! Capability-free by construction: it answers typed calls and exits when told
//! to, so a compromised worker has nothing to escalate with.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate ostd;

api::declare_manifest!(block_io = false, network = false, spawn = false);

api::declare_syscalls![Log, Exit, Send, Recv, RecvTimeout];

ostd::cell_main!(cell_main);

fn cell_main() {
    app_backend::run_worker()
}
