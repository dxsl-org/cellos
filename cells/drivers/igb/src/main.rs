//! Intel `igb` (i210/i211) NIC Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! First NIC family after the 82540EM. The shared `nic_driver` skeleton:
//!   1. Calls `sys_find_pcie_device_by_vendor(8086, id)` for each ID in
//!      `identity::QUERY_DEVICE_IDS` to locate **its own** Ethernet endpoint.
//!      The class triple `02:00:00` is shared with `/bin/e1000`, so naming the
//!      controller exactly is what removes the sibling decline-and-retry race.
//!   2. **Confirms the identity is this family** (`identity::classify`) before it
//!      touches MMIO: a 82540EM (or any other family) must not be programmed with
//!      the igb register model. An unknown ID fails closed with a line naming
//!      vendor:device.
//!   3. Claims BAR0; this cell's controller then resets it, reads the MAC from
//!      the NVM, programs the TX/RX descriptor rings and the unicast/multicast
//!      filters, and enables TX/RX — every DMA buffer authorized first.
//!   4. Calls `sys_register_nic_driver()` — **only after a device was found and
//!      brought up** — and serves the same Tx/Rx/GetMac IPC the net service
//!      already speaks to `/bin/e1000`.
//!
//! Register model and the QEMU-vs-datasheet ID split live in `controller.rs` and
//! `identity.rs`.
//!
//! `PcieDriverCap` is granted by init via the reviewed `/bin/igb` launch edge —
//! NOT via a manifest flag (all 8 flag bits in v1 are occupied).

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]
extern crate alloc;

mod controller;
mod identity;
mod sync;

nic_driver::cell!(controller::IgbController);

// ── Capability manifest ───────────────────────────────────────────────────────
// PcieDriverCap is granted by init via the reviewed `/bin/igb` launch edge (not
// a manifest flag). This manifest declares NO privileged flags.
abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

// ── Syscall allowlist ─────────────────────────────────────────────────────────
// `run_app!` emits a manifest but NOT a `VICELL_SYSCALLS` section, so without
// this the kernel leaves the cell at its TCB's `u64::MAX` allowlist. List
// exactly what the cell calls: the AppContext base set, the PCIe device path,
// NIC registration, the try_send reply path, the exit path, and the DMA calls
// behind `DmaBuf::alloc`/`authorize`. `Exit`/`Yield` carry no allowlist bit and
// are always permitted; naming them documents the intent.
abi::declare_syscalls![
    // AppContext event-loop base set.
    Send,
    TrySend,
    Recv,
    TryRecv,
    Log,
    Heartbeat,
    LookupServiceBound,
    GetTime,
    RecvTimeout,
    // PCIe device path: the exact-identity query (shares allowlist bit 50 with
    // the legacy class query, so the signed mask is unchanged).
    FindPcieDeviceByVendor,
    RequestMmio,
    // System NIC-driver role.
    RegisterNicDriver,
    // Clean shutdown returns through the exit path.
    Exit,
    // DMA: `DmaBuf::alloc` -> GrantAlloc, `DmaBuf::authorize` -> GrantDma.
    GrantAlloc,
    GrantDma
];
