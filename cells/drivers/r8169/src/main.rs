//! Realtek RTL8168/8111 and RTL8125 (`r8169`) NIC Driver Cell — Tier-1 Privileged Driver Cell.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

mod controller;
mod dispatch;
mod dma_layout;
#[cfg(test)]
mod fake;
mod identity;
mod phy;
mod regs;
mod rings;

use controller::RtlController;
use dispatch::{handle, NicReply, REPLY_BUF};
use identity::{classify, QUERY_DEVICE_IDS, VENDOR_REALTEK};
use ostd::app::{AppContext, AppEvent};
use ostd::io::print_fmt;
use ostd::mmio;
use ostd::service::{sender_is_live_provider, service};
use ostd::sync::Mutex;
use ostd::syscall::{
    sys_find_pcie_device_by_vendor, sys_register_nic_driver, sys_try_send, PcieDeviceInfo,
};
use wire::proto::nic::NicStatus;

const RTL_BAR0_LEN: usize = 0x1000;
const RTL_MMIO_MIN: usize = 0x0100;
const FIND_ATTEMPTS: usize = 200;

struct NicState {
    ctrl: RtlController,
}

static STATE: Mutex<Option<NicState>> = Mutex::new(None);

fn bring_up() {
    let mut info = PcieDeviceInfo::zeroed();
    let mut found = false;

    'attempts: for _ in 0..FIND_ATTEMPTS {
        for &device_id in QUERY_DEVICE_IDS.iter() {
            if sys_find_pcie_device_by_vendor(VENDOR_REALTEK, device_id, &mut info).is_ok() {
                found = true;
                break 'attempts;
            }
        }
        ostd::task::yield_now();
    }

    if !found {
        let _ = print_fmt(format_args!(
            "[r8169] no RTL8168/8125 controller present; driver cell idle\n"
        ));
        ostd::syscall::sys_exit(0);
    }

    let Some(identity) = classify(info.vendor_id, info.device_id) else {
        let _ = print_fmt(format_args!(
            "[r8169] unsupported Ethernet {:04x}:{:04x}; driver gate closed\n",
            info.vendor_id, info.device_id
        ));
        ostd::syscall::sys_exit(0);
    };

    let bar0_base = info.bar0_base as usize;
    let bar0_len = if info.bar0_len == 0 {
        RTL_BAR0_LEN
    } else {
        (info.bar0_len as usize).min(RTL_BAR0_LEN)
    };

    if bar0_base == 0 || bar0_len < RTL_MMIO_MIN {
        let _ = print_fmt(format_args!(
            "[r8169] controller has no usable MMIO BAR (base={:#x} len={:#x})\n",
            bar0_base, bar0_len
        ));
        ostd::syscall::sys_exit(1);
    }

    let Ok(mmio_region) = mmio::request_region(bar0_base, bar0_len) else {
        ostd::syscall::sys_exit(1);
    };

    let ctrl = match RtlController::new(mmio_region, info.bdf) {
        Ok(c) => c,
        Err(error) => {
            let _ = print_fmt(format_args!(
                "[r8169] controller initialization failed: {:?}\n",
                error
            ));
            ostd::syscall::sys_exit(1)
        }
    };

    let mac = ctrl.mac;
    let link_up = ctrl.link_up;
    *STATE.lock() = Some(NicState { ctrl });

    let _ = print_fmt(format_args!(
        "[r8169] controller bound {:04x}:{:04x} {} link_up={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
        info.vendor_id, info.device_id, identity.name, link_up,
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    ));

    if let Err(error) = sys_register_nic_driver() {
        *STATE.lock() = None;
        let _ = print_fmt(format_args!(
            "[r8169] NIC driver registration failed: {:?}\n",
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
                reply(&[NicStatus::Failed as u8]);
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
                reply(&[NicStatus::Failed as u8]);
            }
        }
        _ => {}
    }
}

ostd::run_app!(handler);

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
