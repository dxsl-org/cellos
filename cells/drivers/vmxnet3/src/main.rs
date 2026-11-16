//! VMware vmxnet3 NIC Driver Cell — Tier-1 Privileged Driver Cell (S3 phase 09).
//!
//! vmxnet3 (`15AD:07B0`) is the paravirtual NIC VMware recommends; QEMU models
//! it as `-device vmxnet3`. Through the shared `nic_driver` skeleton the cell:
//!   1. Names its controller exactly with `sys_find_pcie_device_by_vendor`
//!      (424) over `identity::QUERY_DEVICE_IDS` and repeats the identity check
//!      (`identity::classify`) before touching MMIO.
//!   2. Reads BAR0 (PT: doorbells, vector masks) and BAR1 (VD: commands) from
//!      the 434 function record and claims both.
//!   3. Negotiates revision 1, resets the device, reads the permanent MAC,
//!      allocates MSI-X through 435 (H-S3-4 b: bounded bring-up, link and
//!      TX-completion waits only), places `DriverShared`, the queue
//!      descriptors, one TX and one RX queue and every frame buffer in one
//!      `GrantDma`-authorized allocation, and activates the device.
//!   4. Registers as the NIC driver only after bring-up and serves the
//!      unchanged Tx/Rx/GetMac wire. RX stays request-driven: the net service
//!      asks for frames, so the RX vector stays masked (`rx=request-driven`).
//!
//! Law 4 exception: `unsafe` is confined to the DMA area (`dma.rs`); MMIO goes
//! through the bounds-checked `ostd::mmio::MmioRegion`.
//!
//! `PcieDriverCap` is granted by init via the reviewed `/bin/vmxnet3` launch
//! edge, not a manifest flag.

// Hosted `cargo test` gets std and libtest's entry, so the register, layout and
// ring logic run as host unit tests; Cell targets stay no_std.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

mod controller;
mod datapath;
mod dma;
mod identity;
mod irq;
mod regs;
mod rings;
mod shared;

nic_driver::cell!(controller::Vmxnet3);

// PcieDriverCap comes from the reviewed launch edge; no privileged manifest flag.
abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

// Exactly what the cell calls: `/bin/e1000e`'s set (AppContext base, exact-ID
// PCIe query, BAR claims, NIC role, exit, DMA grant and free, MSI-X and the
// bounded IRQ park) plus the 434 function record for BAR0/BAR1.
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
    QueryPcieFunction,
    RequestMmio,
    AllocPcieIrq,
    WaitCompletion,
    RegisterNicDriver,
    Exit,
    GrantAlloc,
    GrantDma,
    GrantFree
];
