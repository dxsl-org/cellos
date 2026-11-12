// SPDX-License-Identifier: MIT
//! Chrome: Chromium in a Tier 3 Linux guest, shown in a desktop window.
//!
//! This Cell draws no browser of its own. It owns the window, spawns its own
//! hypervisor (`/bin/hypervisor app`), and shows the guest's screen: the
//! hypervisor shares its scanout Grant and posts damaged rectangles, which are
//! copied into the window. The window's input is forwarded to the guest's
//! virtio tablet/keyboard, a resize asks the guest for a new display size, and
//! closing the window ends the guest (ADR-0017 §5d). Until the guest's first
//! frame, and after it stops, the window says so.

#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]
// The window loop (`app`) is not built for host tests; what only it uses is dead there.
#![cfg_attr(test, allow(dead_code))]

#[cfg(test)]
extern crate std;

extern crate alloc;

#[cfg(not(test))]
mod app;
mod frame;
mod guest;
mod input_bridge;

api::declare_manifest!(block_io = true, network = true, spawn = true);
api::declare_syscalls![
    Log,
    SpawnFromPath,
    StateStash,
    GrantAlloc,
    GrantRegister,
    GrantShare,
    GrantSlice,
    GrantUnregister,
    RecvTimeout,
    SendGather,
    Send,
    Recv,
    TryRecv,
    LookupService,
    GpuGetResolution,
    GetTime,
    SetTimer
];

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(4 * 1024 * 1024);

#[cfg(not(test))]
use app::cell_main;
#[cfg(not(test))]
ostd::cell_main!(cell_main);
