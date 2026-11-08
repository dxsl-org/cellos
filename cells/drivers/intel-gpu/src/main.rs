//! Intel integrated GPU (Xe2/Xe3) Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! Serves the Linux `xe` uAPI over Cellos IPC (ADR-0033) for Panther Lake and
//! Lunar Lake integrated GPUs.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

mod controller;
#[cfg(test)]
mod fake;
mod forcewake;
mod ggtt;
mod guc;
mod identity;
mod regs;

#[cfg(test)]
mod tests;

use controller::IntelGpuController;
use identity::VENDOR_INTEL;
use ostd::app::{AppContext, AppEvent};
use ostd::io::print_fmt;
use ostd::mmio;
use ostd::sync::Mutex;
use ostd::syscall::{
    sys_find_pcie_device_by_vendor, sys_register_service, PcieDeviceInfo, SyscallResult,
};

/// GT MMIO BAR0 size (8 MiB on Xe2/Xe3).
const BAR0_LEN: usize = 0x80_0000;
const FIND_ATTEMPTS: usize = 200;

static STATE: Mutex<Option<IntelGpuController<mmio::MmioRegion>>> = Mutex::new(None);

/// Query IDs for Intel graphics controllers (Panther Lake / Lunar Lake / Wildcat Lake).
pub const QUERY_DEVICE_IDS: &[u16] = &[
    0xB080, 0xB081, 0xB082, 0xB083, 0xB090, 0xB0A0, 0xB0B0, // PTL
    0x6420, 0x64A0, 0x64B0, // LNL
];

fn bring_up() {
    let mut info = PcieDeviceInfo::zeroed();
    let mut found = false;

    'attempts: for _ in 0..FIND_ATTEMPTS {
        for &dev_id in QUERY_DEVICE_IDS {
            if sys_find_pcie_device_by_vendor(VENDOR_INTEL, dev_id, &mut info).is_ok() {
                found = true;
                break 'attempts;
            }
        }
        ostd::syscall::sys_yield();
    }

    if !found {
        let _ = print_fmt(format_args!(
            "[intel-gpu] probe: no matching Intel GPU found on PCIe\n"
        ));
        ostd::syscall::sys_exit(1);
    }

    let bar0_base = info.bar0_base as usize;
    let bar0_len = match info.bar0_len as usize {
        0 => BAR0_LEN,
        len => len.min(BAR0_LEN),
    };
    if bar0_base == 0 {
        let _ = print_fmt(format_args!(
            "[intel-gpu] probe: invalid BAR0: base=0x{:x} len=0x{:x}\n",
            bar0_base, bar0_len
        ));
        ostd::syscall::sys_exit(1);
    }

    let mmio = match mmio::request_region(bar0_base, bar0_len) {
        Ok(region) => region,
        Err(e) => {
            let _ = print_fmt(format_args!(
                "[intel-gpu] probe: failed to map BAR0: {:?}\n",
                e
            ));
            ostd::syscall::sys_exit(1);
        }
    };

    let controller = match IntelGpuController::probe(mmio) {
        Ok(ctrl) => ctrl,
        Err(e) => {
            let _ = print_fmt(format_args!(
                "[intel-gpu] probe: GT initialization failed: {:?}\n",
                e
            ));
            ostd::syscall::sys_exit(1);
        }
    };

    let _ = print_fmt(format_args!(
        "[intel-gpu] online: {} (ver {}.{:02}.{}) clock={}MHz dss_mask=0x{:x}\n",
        controller.identity.name(),
        controller.identity.ip_ver_major,
        controller.identity.ip_ver_minor,
        controller.identity.ip_ver_rev,
        controller.identity.ref_clock_hz / 1_000_000,
        controller.identity.dss_geometry_mask,
    ));

    // Register service::XE_GPU = 21
    if !matches!(
        sys_register_service(api::syscall::service::XE_GPU, 0),
        SyscallResult::Ok(_)
    ) {
        let _ = print_fmt(format_args!(
            "[intel-gpu] failed to register XE_GPU service\n"
        ));
        ostd::syscall::sys_exit(1);
    }

    *STATE.lock() = Some(controller);
}

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => bring_up(),
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => {
            if let Some(mut ctrl) = STATE.lock().take() {
                let _ = ctrl.shutdown();
            }
            ostd::syscall::sys_exit(0);
        }
        _ => {}
    }
}

ostd::run_app!(handler);

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
    RegisterService,
    Exit,
    MapDmaExport
];
