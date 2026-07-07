//! VirtIO-blk Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! Probes VirtIO MMIO slots for a Block device (type 2), claims exclusive MMIO
//! via `sys_request_mmio`, initialises the virtqueue via `virtio-drivers`,
//! registers as the system block driver via `sys_register_block_driver`, then
//! serves the `DrvRequest` sector-I/O protocol (identical wire format to the
//! NVMe Driver Cell — VFS speaks ONE block protocol). Replaces the kernel's
//! `virtio_blk.rs` driver per the Kernel Boundary Law (G2 loader redesign).
//!
//! # Coexistence during migration (phase 02)
//! The single QEMU virtio-blk device is still owned by the kernel (boot + VFS)
//! until phases 05/06. `device::find_and_init_blk` therefore SKIPS any block
//! device already showing `DRIVER_OK` (kernel-owned) and this cell exits
//! gracefully when no free device remains — it never resets the live boot disk.
//! Once the kernel relinquishes the device (phase 06), the same probe claims it.
//!
//! # Law 4 exception
//! `src/device.rs` uses `unsafe` for the `virtio_drivers::Hal` impl and the MMIO
//! probe reads. Every other module is `#![forbid(unsafe_code)]`.

#![no_std]
#![no_main]

extern crate alloc;

mod device;
mod dispatch;

use ostd::app::{AppContext, AppEvent};
use ostd::sync::Mutex;
use ostd::syscall::{sys_register_block_driver, sys_send};
use device::BlkDevice;

static STATE: Mutex<Option<BlkDevice>> = Mutex::new(None);

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            let Some(dev) = device::find_and_init_blk() else {
                // No free VirtIO-blk device: the kernel owns the boot disk until
                // phases 05/06 remove the in-kernel driver, or no device exists on
                // this platform (e.g. x86 uses the NVMe cell). Exit cleanly — the
                // kernel block path stays authoritative.
                ostd::io::println("[virtio-blk] no free device (kernel-owned) — exiting");
                ostd::syscall::sys_exit(0);
            };

            // Announce as the active block driver: VFS then routes DrvRequest IPC here.
            let _ = sys_register_block_driver();

            *STATE.lock() = Some(dev);
            ostd::io::println("[virtio-blk] ready");
        }

        // VFS speaks the raw DrvRequest protocol (no 0xAC App-SDK envelope), so
        // requests may arrive as RawMessage; accept Message too — layout is identical.
        AppEvent::Message { sender_tid, data }
        | AppEvent::RawMessage { sender_tid, data } => {
            let mut reply = [0u8; dispatch::REPLY_SIZE];
            let len = if let Some(dev) = STATE.lock().as_mut() {
                dispatch::handle(dev, data.as_ref(), &mut reply)
            } else {
                reply[0] = 1; // not initialised
                1
            };
            // Blocking send: the VFS client issues sys_send + a blocking sys_recv(tid),
            // so it is guaranteed to be waiting (matches the NVMe cell reply path).
            let _ = sys_send(sender_tid, &reply[..len]);
        }

        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => {
            ostd::syscall::sys_exit(0);
        }

        _ => {}
    }
}

ostd::run_app!(handler);

// PcieDriverCap is granted by loader.rs via path match (/bin/block). This
// manifest declares NO privileged flags — init/loader elevates the cell at spawn.
api::declare_manifest!(
    block_io = false, network = false, spawn = false,
    gpio = false, uart = false, hypervisor = false
);
