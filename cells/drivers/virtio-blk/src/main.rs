//! VirtIO-blk Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! riscv64 / aarch64: probes VirtIO MMIO slots for a Block device (type 2),
//! claims exclusive MMIO via `sys_request_mmio` and publishes
//! `BlockDevice::Mmio { base }`. x86_64: claims a virtio-blk PCI function by
//! vendor:device (`1af4:1042`, then `1af4:1001`), drives it through
//! `virtio-pci-transport` with polled queues and publishes
//! `BlockDevice::Pci { bdf }`. Either way the virtqueue is initialised via
//! `virtio-drivers` and the cell serves the `DrvRequest` sector-I/O protocol
//! (identical wire format to the NVMe Driver Cell — VFS speaks ONE block
//! protocol) to the registered VFS only. Replaces the kernel's `virtio_blk.rs`
//! driver per the Kernel Boundary Law (G2 loader redesign).
//!
//! # Coexistence during migration (phase 02)
//! The single QEMU virtio-blk device is still owned by the kernel (boot + VFS)
//! until phases 05/06. `device::find_and_init_blk` therefore SKIPS any block
//! device already showing `DRIVER_OK` (kernel-owned) and this cell exits
//! gracefully when no free device remains — it never resets the live boot disk.
//! Once the kernel relinquishes the device (phase 06), the same probe claims it.
//!
//! # Law 4 exception
//! `src/mmio.rs`, `src/hal_x86.rs` and `src/io_x86.rs` use `unsafe` for the
//! `virtio_drivers::Hal` impls, DMA bounce copies, the non-blocking request
//! calls and the MMIO probe reads. No other module does.

#![no_std]
#![no_main]

extern crate alloc;

#[cfg(target_arch = "x86_64")]
mod bars_x86;
mod device;
mod dispatch;
#[cfg(target_arch = "x86_64")]
mod hal_x86;
#[cfg(target_arch = "x86_64")]
mod io_x86;
#[cfg(not(target_arch = "x86_64"))]
mod mmio;
#[cfg(target_arch = "x86_64")]
mod pci_x86;
#[cfg(target_arch = "x86_64")]
mod wait;

use core::sync::atomic::{AtomicUsize, Ordering};
use device::BlkDevice;
use ostd::app::{AppContext, AppEvent};
use ostd::sync::Mutex;
use ostd::syscall::{sys_lookup_service, sys_register_block_device, sys_send, BlockDevice};

static STATE: Mutex<Option<BlkDevice>> = Mutex::new(None);

/// Tid of the registered VFS once a request from it was accepted. Tids are never
/// reissued, so a cached match stays valid for the life of that VFS.
static VFS_TID: AtomicUsize = AtomicUsize::new(0);

/// Whether `sender_tid` is the registered VFS, the only accepted block client.
fn sender_is_vfs(sender_tid: usize) -> bool {
    let cached = VFS_TID.load(Ordering::Relaxed);
    if cached != 0 && cached == sender_tid {
        return true;
    }
    if sys_lookup_service(api::syscall::service::VFS) == Some(sender_tid) {
        VFS_TID.store(sender_tid, Ordering::Relaxed);
        return true;
    }
    false
}

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            let Some(dev) = device::find_and_init_blk() else {
                // No free VirtIO-blk device: the kernel owns the boot disk until
                // phases 05/06 remove the in-kernel driver, or no device exists on
                // this platform. Exit cleanly — the kernel block path stays
                // authoritative. On x86_64 the PCI probe already logged why.
                #[cfg(not(target_arch = "x86_64"))]
                ostd::io::println("[virtio-blk] no free device (kernel-owned) — exiting");
                ostd::syscall::sys_exit(0);
            };

            // Publish state before registration because VFS may route a request
            // immediately after successful publication.
            let route = dev.route();
            *STATE.lock() = Some(dev);
            if let Err(error) = sys_register_block_device(route) {
                *STATE.lock() = None;
                let device = match route {
                    BlockDevice::Mmio { base } => alloc::format!("base={base:#x}"),
                    BlockDevice::Pci { bdf } => alloc::format!("bdf={bdf:#06x}"),
                };
                ostd::io::println(&alloc::format!(
                    "[virtio-blk] block driver registration failed: {error:?} ({device})"
                ));
                ostd::syscall::sys_exit(1);
            }
            ostd::io::println("[virtio-blk] ready");
        }

        // VFS speaks the raw DrvRequest protocol (no 0xAC App-SDK envelope), so
        // requests may arrive as RawMessage; accept Message too — layout is identical.
        // Only the registered VFS may issue block requests; any other sender gets
        // `NotIssued` without touching the device.
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
            let mut reply = [0u8; dispatch::REPLY_SIZE];
            let len = if !sender_is_vfs(sender_tid) {
                let _ = ostd::io::print_fmt(format_args!(
                    "[virtio-blk] refused block request from tid={}: sender is not the registered VFS\n",
                    sender_tid
                ));
                reply[0] = types::block_wire::BlockOutcome::NotIssued.to_byte();
                1
            } else if let Some(dev) = STATE.lock().as_mut() {
                dispatch::handle(dev, data.as_ref(), &mut reply)
            } else {
                reply[0] = types::block_wire::BlockOutcome::NotIssued.to_byte(); // not initialised
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
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);
