//! VirtIO NIC Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! Finds a virtio Network device, initialises the virtqueues via `virtio-drivers`,
//! registers as the system NIC via `sys_register_nic_driver`, then serves Tx/Rx/GetMac
//! IPC from the net service.  This cell replaces the kernel's `virtio_net.rs` driver
//! per the Kernel Boundary Law (2026-06-23).
//!
//! # Transports
//! - riscv64 / aarch64 (`mmio`): probes the QEMU virt VirtIO MMIO slots and claims
//!   the slot via `sys_request_mmio`.
//! - x86_64 (`pci_x86`): claims a `1af4:1041` (modern) or `1af4:1000` (transitional)
//!   PCI function, drives it through `virtio-pci-transport` with DMA authorised for
//!   its BDF, and polls (no interrupts until phase 04b).
//!
//! # Fallback
//! If no VirtIO NIC is found, or the PCI function is refused (logged by name), the
//! cell exits cleanly (code 0).  The e1000/igb Driver Cells remain.
//!
//! # Law 4 exception
//! `src/mmio.rs` and `src/pci_x86/hal.rs` use `unsafe` for the `virtio_drivers::Hal`
//! impls, DMA bounce copies and MMIO probe reads; `src/pci_x86/raw_net.rs` for the
//! raw queue calls that borrow buffers across the device's ownership window. All
//! other modules are safe code.

#![no_std]
#![no_main]

extern crate alloc;

mod device;
mod dispatch;
#[cfg(not(target_arch = "x86_64"))]
mod mmio;
#[cfg(target_arch = "x86_64")]
mod pci_x86;

use device::NetDevice;
use dispatch::{handle, NicReply, REPLY_BUF};
use ostd::app::{AppContext, AppEvent};
use ostd::service::{sender_is_live_provider, service};
use ostd::sync::Mutex;
use ostd::syscall::{sys_register_nic_driver, sys_try_send};
use wire::proto::nic::NicStatus;

static STATE: Mutex<Option<NetDevice>> = Mutex::new(None);

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            // Find and initialise the platform's virtio Network device.
            let Some(dev) = device::find_and_init_net() else {
                // No usable virtio NIC on this platform — exit gracefully.
                // The e1000/igb Driver Cells handle the other PCIe NICs.
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
            // Only the live net service may drive the NIC; any other cell could
            // inject or read raw frames. `sender_tid` is kernel-stamped and the
            // registry is asked per request, so a restarted net service — or one
            // init has not registered yet — is followed without a cached tid.
            // The refusal is the ordinary failure byte the net service retries on.
            if !sender_is_live_provider(service::NET, sender_tid) {
                reply(&[NicStatus::Failed as u8]);
                return;
            }
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
                reply(&[NicStatus::Failed as u8]);
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
abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);
