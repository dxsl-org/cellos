//! Intel e1000 (8254x) NIC Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! This cell owns one 82540EM or 82545EM PCIe endpoint exclusively. Through the
//! shared `nic_driver` skeleton it:
//!   1. Calls `sys_find_pcie_device_by_vendor(8086, id)` (opcode 424) for each ID
//!      in `identity::QUERY_DEVICE_IDS`. The class triple `02:00:00` is shared
//!      with `/bin/igb`, `/bin/e1000e` and `/bin/virtio-net`, so the cell names
//!      its controllers exactly instead of using the class query (H-S3-6).
//!      Absent, it exits silently.
//!   2. Confirms the identity (`identity::classify`) before it touches MMIO.
//!   3. Claims exclusive MMIO via `sys_request_mmio`.
//!   4. Initialises the controller (TX/RX ring setup, EEPROM MAC read).
//!   5. Calls `sys_register_nic_driver()` to announce itself to the kernel.
//!   6. Serves Tx/Rx/GetMac IPC from the net cell. RX is request-driven (the net
//!      service asks for frames), so the cell stays polled.
//!
//! This cell is granted `PcieDriverCap` by `init` via direct TCB write at spawn
//! time — NOT via a manifest flag (all 8 flag bits in v1 are occupied).

#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate alloc;

mod controller;
mod identity;

nic_driver::cell!(controller::E1000Controller);

// ── Capability manifest ───────────────────────────────────────────────────────
// PcieDriverCap is granted by init via direct TCB write (not a manifest flag).
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
// this the kernel leaves the cell at its TCB's `u64::MAX` allowlist. The list is
// exactly `/bin/igb`'s: the AppContext base set, the exact-identity PCIe query,
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
    // PCIe device path: the exact-identity query (424, shares allowlist bit 50
    // with the legacy class query) and the BAR0 claim.
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
