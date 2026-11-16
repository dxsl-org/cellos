//! Intel `e1000e` (82574L) NIC Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! The 82574L is QEMU's `e1000e`, VMware's "e1000e" and q35's default NIC; it is
//! also the base phase 13 extends to I219 (`identity::MacKind::PchLan`). Through
//! the shared `nic_driver` skeleton it:
//!   1. Names its controller exactly with `sys_find_pcie_device_by_vendor`
//!      (424) over `identity::QUERY_DEVICE_IDS`; `02:00:00` is shared with
//!      `/bin/e1000`, `/bin/igb` and `/bin/virtio-net`.
//!   2. Confirms the identity (`identity::classify`) before touching MMIO;
//!      unknown and `Target` IDs fail closed by name.
//!   3. Claims BAR0, resets the controller, reads the station address,
//!      allocates MSI-X through 435 (H-S3-4 b: bounded bring-up, link and
//!      TX-completion waits only), restarts PHY autoneg over `MDIC`, programs
//!      legacy TX/RX rings whose every buffer is `GrantDma`-authorized, and
//!      enables TX/RX.
//!   4. Registers as the NIC driver only after bring-up and serves the
//!      unchanged Tx/Rx/GetMac wire. RX stays request-driven: the net service
//!      asks for frames, so no RX interrupt is used (`rx=request-driven`).
//!
//! DMA rings and buffers go through `nic_driver::{dma, ring}` and MMIO through
//! the bounds-checked `ostd::mmio::MmioRegion`, so the cell holds no `unsafe`.
//!
//! `PcieDriverCap` is granted by init via the reviewed `/bin/e1000e` launch edge,
//! not a manifest flag.

// Hosted `cargo test` gets std and libtest's entry, so the register, PHY,
// descriptor and IVAR logic run as host unit tests; Cell targets stay no_std.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]
extern crate alloc;

mod controller;
#[cfg(test)]
mod fake;
mod identity;
mod irq;
mod pch;
mod phy;
mod regs;
mod reset;
mod rings;

nic_driver::cell!(controller::E1000eController);

// PcieDriverCap comes from the reviewed launch edge; no privileged manifest flag.
abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

// Exactly what the cell calls: `/bin/igb`'s set (AppContext base, exact-ID PCIe
// query, BAR0 claim, NIC role, exit, DMA grants) plus the MSI-X allocation and
// the bounded IRQ park of `irq.rs`. Without this section the kernel would leave
// the TCB's `u64::MAX` allowlist in place.
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
    AllocPcieIrq,
    WaitCompletion,
    RegisterNicDriver,
    Exit,
    GrantAlloc,
    GrantDma
];
