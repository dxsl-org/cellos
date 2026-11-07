//! Broadcom BCM5719/BCM5720 (`tg3`/`bge`) NIC Driver Cell — Tier-1 Privileged Driver Cell.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

mod ape;
mod controller;
mod dispatch;
mod dma_layout;
#[cfg(test)]
mod fake;
mod identity;
mod phy;
mod regs;
mod rings;

use controller::BcmController;
use dispatch::{handle, NicReply, REPLY_BUF};
use identity::{classify, QUERY_DEVICE_IDS, VENDOR_BROADCOM};
use ostd::app::{AppContext, AppEvent};
use ostd::io::print_fmt;
use ostd::mmio;
use ostd::service::{sender_is_live_provider, service};
use ostd::sync::Mutex;
use ostd::syscall::{
    sys_find_pcie_device_by_vendor, sys_register_nic_driver, sys_try_send, PcieDeviceInfo,
};

const BCM_BAR0_LEN: usize = 0x1_0000;
const BCM_MMIO_MIN: usize = 0x6808;
const FIND_ATTEMPTS: usize = 200;

struct NicState {
    ctrl: BcmController,
}

static STATE: Mutex<Option<NicState>> = Mutex::new(None);

fn bring_up() {
    let mut info = PcieDeviceInfo::zeroed();
    let mut found = false;

    'attempts: for _ in 0..FIND_ATTEMPTS {
        for &device_id in QUERY_DEVICE_IDS.iter() {
            if sys_find_pcie_device_by_vendor(VENDOR_BROADCOM, device_id, &mut info).is_ok() {
                found = true;
                break 'attempts;
            }
        }
        ostd::task::yield_now();
    }

    if !found {
        let _ = print_fmt(format_args!(
            "[bcm57xx] no BCM5719/BCM5720 controller present; driver cell idle\n"
        ));
        ostd::syscall::sys_exit(0);
    }

    let Some(identity) = classify(info.vendor_id, info.device_id) else {
        let _ = print_fmt(format_args!(
            "[bcm57xx] unsupported Ethernet {:04x}:{:04x}; driver gate closed\n",
            info.vendor_id, info.device_id
        ));
        ostd::syscall::sys_exit(0);
    };

    let bar0_base = info.bar0_base as usize;
    let bar0_len = if info.bar0_len == 0 {
        BCM_BAR0_LEN
    } else {
        (info.bar0_len as usize).min(BCM_BAR0_LEN)
    };

    if bar0_base == 0 || bar0_len < BCM_MMIO_MIN {
        let _ = print_fmt(format_args!(
            "[bcm57xx] controller has no usable MMIO BAR (base={:#x} len={:#x})\n",
            bar0_base, bar0_len
        ));
        ostd::syscall::sys_exit(1);
    }

    let Ok(mmio_region) = mmio::request_region(bar0_base, bar0_len) else {
        ostd::syscall::sys_exit(1);
    };

    let ctrl = match BcmController::new(mmio_region, info.bdf) {
        Ok(c) => c,
        Err(error) => {
            let _ = print_fmt(format_args!(
                "[bcm57xx] controller initialization failed: {:?}\n",
                error
            ));
            ostd::syscall::sys_exit(1)
        }
    };

    let mac = ctrl.mac;
    let link_up = ctrl.link_up;
    *STATE.lock() = Some(NicState { ctrl });

    let _ = print_fmt(format_args!(
        "[bcm57xx] controller bound {:04x}:{:04x} {} link_up={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
        info.vendor_id, info.device_id, identity.name, link_up,
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    ));

    if let Err(error) = sys_register_nic_driver() {
        *STATE.lock() = None;
        let _ = print_fmt(format_args!(
            "[bcm57xx] NIC driver registration failed: {:?}\n",
            error
        ));
        ostd::syscall::sys_exit(1);
    }
}

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => bring_up(),
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
            let operation = ostd::ipc::current();
            let reply = |bytes: &[u8]| {
                if let Some(operation) = operation {
                    let _ = ostd::ipc::reply(operation, bytes);
                } else {
                    let _ = sys_try_send(sender_tid, bytes);
                }
            };
            if !sender_is_live_provider(service::NET, sender_tid) {
                reply(&[1u8]);
                return;
            }
            let mut out_buf = [0u8; REPLY_BUF];
            if let Some(state) = STATE.lock().as_mut() {
                match handle(&mut state.ctrl, data.as_ref(), &mut out_buf) {
                    NicReply::Status(code) => {
                        reply(&[code]);
                    }
                    NicReply::Frame { len, buf } => {
                        reply(&buf[..2 + len]);
                    }
                    NicReply::Raw(mac) => {
                        reply(&mac);
                    }
                }
            } else {
                reply(&[1u8]);
            }
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
    RegisterNicDriver,
    Exit,
    GrantAlloc,
    GrantDma
];
