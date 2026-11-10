//! Intel integrated GPU (Xe2/Xe3) Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! Serves the Linux `xe` uAPI over Cellos IPC (ADR-0033) for Panther Lake and Lunar Lake
//! integrated GPUs as service `XE_GPU` (21), which init registers for this Cell (CP1 D-G8).
//! Bring-up: forcewake -> GT identity -> GuC firmware -> GGTT -> GuC load -> serve.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]
extern crate alloc;

mod batch;
mod bring_up;
mod cell;
mod controller;
#[cfg(test)]
mod fake;
mod forcewake;
mod ggtt;
mod guc;
mod identity;
mod owner_watch;
mod ppgtt_store;
mod regs;
mod service;

#[cfg(test)]
mod bring_up_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod tests;

use cell::cell_main;

ostd::cell_main!(cell_main);

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

api::declare_syscalls![
    Send,
    TrySend,
    Recv,
    TryRecv,
    Reply,
    Log,
    Heartbeat,
    LookupService,
    GetTime,
    RecvTimeout,
    FindPcieDeviceByVendor,
    RequestMmio,
    AllocPcieIrq,
    WaitCompletion,
    Exit,
    MapDmaExport,
    IpcCurrent,
    IpcReply,
    WatchCellOwner,
    CancelCellOwnerWatch,
    OpenCap,
    ReadCap,
    CloseCap,
    GrantAlloc,
    GrantSlice,
    GrantFree,
    GrantDma
];
