//! Intel `e1000e` (82574L) NIC Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! The 82574L is QEMU's `e1000e`, VMware's "e1000e" and q35's default NIC; it is
//! also the base phase 13 extends to I219 (`identity::MacKind::PchLan`). It:
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
//! Law 4 exception: `unsafe` is confined to DMA ring/buffer access (`rings.rs`)
//! and fixed-array construction (`dma_layout.rs`); MMIO goes through the
//! bounds-checked `ostd::mmio::MmioRegion`.
//!
//! `PcieDriverCap` is granted by init via the reviewed `/bin/e1000e` launch edge,
//! not a manifest flag.

// Hosted `cargo test` gets std and libtest's entry, so the register, PHY,
// descriptor and IVAR logic run as host unit tests; Cell targets stay no_std.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

mod controller;
mod dispatch;
mod dma_layout;
#[cfg(test)]
mod fake;
mod identity;
mod irq;
mod phy;
mod regs;
mod reset;
mod rings;

use controller::E1000eController;
use identity::{classify, QUERY_DEVICE_IDS, VENDOR_INTEL};
use ostd::app::{AppContext, AppEvent};
use ostd::io::print_fmt;
use ostd::mmio;
use ostd::sync::Mutex;
use ostd::syscall::{sys_find_pcie_device_by_vendor, sys_register_nic_driver, PcieDeviceInfo};

/// BAR0 register aperture of the 82574 (128 KiB).
const BAR0_LEN: usize = 0x2_0000;
/// Bounded wait for the ECAM scan to publish the device table (as `/bin/igb`).
const FIND_ATTEMPTS: usize = 200;

static STATE: Mutex<Option<E1000eController>> = Mutex::new(None);

fn bring_up() {
    let mut info = PcieDeviceInfo::zeroed();
    let mut found = false;
    'attempts: for _ in 0..FIND_ATTEMPTS {
        for &device_id in QUERY_DEVICE_IDS.iter() {
            if let Ok(true) = sys_find_pcie_device_by_vendor(VENDOR_INTEL, device_id, &mut info) {
                found = true;
                break 'attempts;
            }
        }
        ostd::task::yield_now();
    }
    if !found {
        let _ = print_fmt(format_args!(
            "[e1000e] no 82574 controller present; driver cell idle\n"
        ));
        ostd::syscall::sys_exit(0);
    }

    // Defense in depth behind the kernel's family gate.
    let Some(identity) = classify(info.vendor_id, info.device_id) else {
        let _ = print_fmt(format_args!(
            "[e1000e] unsupported Ethernet {:04x}:{:04x}; driver gate closed\n",
            info.vendor_id, info.device_id
        ));
        ostd::syscall::sys_exit(0);
    };

    let bar0_base = info.bar0_base as usize;
    let bar0_len = match info.bar0_len as usize {
        0 => BAR0_LEN,
        len => len.min(BAR0_LEN),
    };
    if bar0_base == 0 || bar0_len < regs::MMIO_MIN {
        let _ = print_fmt(format_args!(
            "[e1000e] controller has no usable MMIO BAR (base={:#x} len={:#x})\n",
            bar0_base, bar0_len
        ));
        ostd::syscall::sys_exit(1);
    }
    let Ok(mmio_region) = mmio::request_region(bar0_base, bar0_len) else {
        ostd::syscall::sys_exit(1);
    };

    let ctrl = match E1000eController::new(mmio_region, info.bdf) {
        Ok(ctrl) => ctrl,
        Err(error) => {
            let _ = print_fmt(format_args!(
                "[e1000e] controller initialization failed: {:?}\n",
                error
            ));
            ostd::syscall::sys_exit(1)
        }
    };

    let (mac, link_up) = (ctrl.mac, ctrl.link_up);
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
        "[e1000e] controller bound {:04x}:{:04x} {} link_up={} mac={} irq={}{} rx=request-driven vectors={} irq_wakes={} park_expiries={}\n",
        info.vendor_id, info.device_id, identity.name, link_up, dispatch::MacFmt(mac),
        irq, why, vectors, wakes, expiries
    ));
    if let Err(error) = sys_register_nic_driver() {
        *STATE.lock() = None;
        let _ = print_fmt(format_args!(
            "[e1000e] NIC driver registration failed: {:?}\n",
            error
        ));
        ostd::syscall::sys_exit(1);
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

// Exactly what the cell calls: `/bin/igb`'s set (AppContext base, exact-ID PCIe
// query, BAR0 claim, NIC role, exit, DMA grants) plus the MSI-X allocation and
// the bounded IRQ park of `irq.rs`. Without this section the kernel would leave
// the TCB's `u64::MAX` allowlist in place.
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
    RegisterNicDriver,
    Exit,
    GrantAlloc,
    GrantDma
];
