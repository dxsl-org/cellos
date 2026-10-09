//! VirtIO NIC Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! Probes VirtIO MMIO slots for a Network device (device type 1), claims exclusive
//! MMIO via `sys_request_mmio`, initialises the virtqueues via `virtio-drivers`,
//! registers as the system NIC via `sys_register_nic_driver`, then serves Tx/Rx/GetMac
//! IPC from the net service.  This cell replaces the kernel's `virtio_net.rs` driver
//! per the Kernel Boundary Law (2026-06-23).
//!
//! # Fallback
//! If no VirtIO MMIO NIC is found (x86_64, real hardware with PCIe-only NIC) the cell
//! exits cleanly (code 0).  The kernel's fallback path and the e1000 Driver Cell remain.
//!
//! # Law 4 exception
//! `src/device.rs` uses `unsafe` for the `virtio_drivers::Hal` impl and MMIO probe reads.
//! All other modules forbid unsafe code.

#![no_std]
#![no_main]

extern crate alloc;

mod device;
mod dispatch;

use device::NetDevice;
use dispatch::{handle, NicReply, REPLY_BUF};
use ostd::app::{AppContext, AppEvent};
use ostd::sync::Mutex;
use ostd::syscall::{sys_register_nic_driver, sys_try_send};

static STATE: Mutex<Option<NetDevice>> = Mutex::new(None);

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            // Probe VirtIO MMIO slots for a Network device.
            let Some(dev) = device::find_and_init_net() else {
                // No VirtIO MMIO NIC on this platform — exit gracefully.
                // The e1000 Driver Cell handles PCIe platforms.
                ostd::syscall::sys_exit(0);
            };

            // Register only after state is ready because successful publication
            // allows the net service to route Tx/Rx immediately.
            *STATE.lock() = Some(dev);
            if sys_register_nic_driver().is_err() {
                *STATE.lock() = None;
                ostd::io::println("[virtio-net] NIC driver registration failed");
                ostd::syscall::sys_exit(1);
            }
            ostd::io::println("[virtio-net] ready");
        }

        // The net service speaks the raw NIC wire protocol (no 0xAC App-SDK
        // envelope), so requests arrive as RawMessage. Accept Message too for
        // envelope-wrapped senders — the dispatch payload layout is identical.
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
            let operation = ostd::ipc::current();
            let reply = |bytes: &[u8]| {
                if let Some(operation) = operation {
                    let _ = ostd::ipc::reply(operation, bytes);
                } else {
                    let _ = sys_try_send(sender_tid, bytes);
                }
            };
            let mut out_buf = [0u8; REPLY_BUF];
            if let Some(dev) = STATE.lock().as_mut() {
                match handle(dev, data.as_ref(), &mut out_buf) {
                    NicReply::Status(code) => {
                        reply(&[code]);
                    }
                    NicReply::Frame { len, buf } => {
                        reply(&buf[..2 + len]);
                    }
                    NicReply::Mac(mac) => {
                        reply(&mac);
                    }
                }
            } else {
                reply(&[1u8]);
            }
        }

        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => {
            ostd::syscall::sys_exit(0);
        }

        _ => {}
    }
}

ostd::run_app!(handler);

// PcieDriverCap is granted by loader.rs via path match (/bin/virtio-net).
api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);
