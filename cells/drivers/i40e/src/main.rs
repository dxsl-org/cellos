//! Intel X710/XL710 (`i40e`) NIC Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! Probe, identity gate, BAR0 claim, registration, the sender gate and the NIC
//! wire come from `nic_driver`; this cell supplies the i40e register model,
//! admin queue, HMC sizing, identity table and descriptor formats.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]
extern crate alloc;

mod aq;
mod controller;
#[cfg(test)]
mod fake;
mod hmc;
mod identity;
mod regs;
mod rings;

nic_driver::cell!(controller::I40eController);

abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

abi::declare_syscalls![
    Send,
    TrySend,
    Recv,
    TryRecv,
    Log,
    Heartbeat,
    LookupServiceBound,
    GetTime,
    RecvTimeout,
    FindPcieDeviceByVendor,
    RequestMmio,
    RegisterNicDriver,
    Exit,
    GrantAlloc,
    GrantDma
];
