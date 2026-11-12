//! VMware vmxnet3 NIC Driver Cell — Tier-1 Privileged Driver Cell (S3 phase 09).
//!
//! vmxnet3 (`15AD:07B0`) is the paravirtual NIC VMware recommends; QEMU models
//! it as `-device vmxnet3`. The cell:
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
mod dispatch;
mod dma;
mod identity;
mod irq;
mod regs;
mod rings;
mod shared;

use api::pcie_function::{PcieBar, BAR_KIND_MEM32, BAR_KIND_MEM64};
use controller::Vmxnet3;
use identity::{classify, QUERY_DEVICE_IDS, VENDOR_VMWARE};
use ostd::app::{AppContext, AppEvent};
use ostd::io::print_fmt;
use ostd::mmio::{self, MmioRegion};
use ostd::sync::Mutex;
use ostd::syscall::{
    sys_find_pcie_device_by_vendor, sys_query_pcie_function, sys_register_nic_driver,
    PcieDeviceInfo,
};

/// Bounded wait for the PCI scan to publish the device table (as `/bin/igb`).
const FIND_ATTEMPTS: usize = 200;

static STATE: Mutex<Option<Vmxnet3>> = Mutex::new(None);

fn fail(what: core::fmt::Arguments<'_>) -> ! {
    let _ = print_fmt(format_args!("[vmxnet3] {what}\n"));
    ostd::syscall::sys_exit(1)
}

/// Claim memory BAR `bar` when it holds at least `min` bytes.
fn claim(index: usize, bar: &PcieBar, min: usize) -> MmioRegion {
    let memory = bar.kind == BAR_KIND_MEM32 || bar.kind == BAR_KIND_MEM64;
    let len = usize::try_from(bar.len).unwrap_or(0);
    if !memory || bar.base == 0 || len < min {
        fail(format_args!(
            "BAR{index} unusable (kind={} base={:#x} len={:#x})",
            bar.kind, bar.base, bar.len
        ));
    }
    match mmio::request_region(bar.base as usize, len) {
        Ok(region) => region,
        Err(error) => fail(format_args!("BAR{index} claim failed: {error:?}")),
    }
}

fn bring_up() {
    let mut info = PcieDeviceInfo::zeroed();
    let mut found = false;
    'attempts: for _ in 0..FIND_ATTEMPTS {
        for &device_id in QUERY_DEVICE_IDS.iter() {
            if let Ok(true) = sys_find_pcie_device_by_vendor(VENDOR_VMWARE, device_id, &mut info) {
                found = true;
                break 'attempts;
            }
        }
        ostd::task::yield_now();
    }
    if !found {
        let _ = print_fmt(format_args!(
            "[vmxnet3] no vmxnet3 controller present; driver cell idle\n"
        ));
        ostd::syscall::sys_exit(0);
    }

    // Defense in depth behind the kernel's family gate.
    let Some(identity) = classify(info.vendor_id, info.device_id) else {
        let _ = print_fmt(format_args!(
            "[vmxnet3] unsupported Ethernet {:04x}:{:04x}; driver gate closed\n",
            info.vendor_id, info.device_id
        ));
        ostd::syscall::sys_exit(0);
    };
    let record = match sys_query_pcie_function(info.bdf) {
        Ok(record) => record,
        Err(error) => fail(format_args!("query {:#x} failed: {error:?}", info.bdf)),
    };
    let pt = claim(0, &record.bars[0], regs::PT_MIN);
    let vd = claim(1, &record.bars[1], regs::VD_MIN);

    let ctrl = match Vmxnet3::new(pt, vd, info.bdf) {
        Ok(ctrl) => ctrl,
        Err(error) => fail(format_args!("controller initialization failed: {error}")),
    };

    let (mac, link_up, mbps) = (ctrl.mac, ctrl.link_up, ctrl.link_mbps);
    let (vectors, reason, wakes, expiries) = ctrl.irq_state();
    // Publish state before registration: the kernel may route requests as soon
    // as the syscall succeeds.
    *STATE.lock() = Some(ctrl);
    let (irq, why) = if vectors > 0 {
        ("msix", "")
    } else {
        ("polled reason=", reason)
    };
    let _ = print_fmt(format_args!(
        "[vmxnet3] controller bound {:04x}:{:04x} {} rev=1 link_up={} speed={}Mb/s mac={} irq={}{} rx=request-driven vectors={} irq_wakes={} park_expiries={}\n",
        info.vendor_id, info.device_id, identity.name, link_up, mbps, dispatch::MacFmt(mac),
        irq, why, vectors, wakes, expiries
    ));
    if let Err(error) = sys_register_nic_driver() {
        let ctrl = STATE.lock().take();
        drop(ctrl); // quiesces the device
        fail(format_args!("NIC driver registration failed: {error:?}"));
    }
}

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => bring_up(),
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
            dispatch::serve(&STATE, sender_tid, data.as_ref());
        }
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => {
            ostd::syscall::sys_exit(0);
        }
        _ => {}
    }
}

ostd::run_app!(handler);

// PcieDriverCap comes from the reviewed launch edge; no privileged manifest flag.
api::declare_manifest!(
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
