//! Intel integrated GPU (Xe2/Xe3) Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! Serves the Linux `xe` uAPI over Cellos IPC (ADR-0033) for Panther Lake and Lunar Lake
//! integrated GPUs as service `XE_GPU` (21), which init registers for this Cell (CP1 D-G8).
//! Bring-up: forcewake -> GT identity -> GuC firmware -> GGTT -> WOPCM -> GuC reset ->
//! ADS/params -> upload -> CT -> serve. Client buffers are client memory exported to this
//! driver (437) and mapped into its device domain (438); the driver revokes them (439)
//! after clearing their GPU mappings.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]
extern crate alloc;

#[cfg(test)]
mod batch;
mod bring_up;
mod cell;
mod clock;
mod controller;
mod devmem;
#[cfg(test)]
mod fake;
mod forcewake;
mod ggtt;
mod gpu;
mod guc;
mod guc_ct;
mod identity;
mod owner_watch;
mod ppgtt_store;
mod regs;
mod service;

#[cfg(test)]
mod bring_up_tests;
#[cfg(test)]
mod gpu_tests;
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
    RevokeDmaExport,
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
