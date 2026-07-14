//! Driver Cell registration — tracks which cells own the block, NIC, and GPU roles.
//!
//! When a Tier-1 Driver Cell calls `sys_register_block_driver`,
//! `sys_register_nic_driver`, or `sys_register_gpu_driver`, the kernel records
//! its TID here.  Service clients use `sys_lookup_service(service::X)` to find
//! the provider; these statics are the backing store for that lookup.
//!
//! `0` means "no driver cell registered". Block + NIC fall back to kernel-resident drivers
//! (virtio_blk / MMC for block; no NIC fallback — NIC is always a Driver Cell).
//! GPU has no kernel fallback; compositor refuses to init until a GPU Cell registers.

use core::sync::atomic::{AtomicUsize, Ordering};

/// TID of the registered block Driver Cell (0 = none; kernel virtio_blk/MMC is the fallback).
pub static BLOCK_DRIVER_CELL: AtomicUsize = AtomicUsize::new(0);

/// TID of the registered NIC Driver Cell (0 = none; no kernel NIC fallback exists).
pub static NIC_DRIVER_CELL: AtomicUsize = AtomicUsize::new(0);

/// TID of the registered GPU Driver Cell (0 = none; no kernel GPU fallback).
pub static GPU_DRIVER_CELL: AtomicUsize = AtomicUsize::new(0);

/// Record `tid` as the active block driver.  Overwrites any previous registration.
///
/// Logged at `warn!` for the same reason as `set_input_cell`: Driver Cells are
/// spawned by init AFTER the kernel drops its log level to Warn, this is a
/// one-time boot-integrity event (the kernel now routes all sector I/O to this
/// TID), and it is the marker the x86 nvme/nic integration tests assert on.
pub fn register_block_driver(tid: usize) {
    BLOCK_DRIVER_CELL.store(tid, Ordering::Release);
    log::warn!("[driver_cell] block driver registered: tid={}", tid);
}

/// Record `tid` as the active NIC driver.  Overwrites any previous registration.
/// `warn!` — see `register_block_driver`.
pub fn register_nic_driver(tid: usize) {
    NIC_DRIVER_CELL.store(tid, Ordering::Release);
    log::warn!("[driver_cell] NIC driver registered: tid={}", tid);
}

/// Clear the block driver registration (called on cell exit/kill).
pub fn deregister_block_driver(tid: usize) {
    BLOCK_DRIVER_CELL
        .compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed)
        .ok();
}

/// Clear the NIC driver registration (called on cell exit/kill).
pub fn deregister_nic_driver(tid: usize) {
    NIC_DRIVER_CELL
        .compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed)
        .ok();
}

/// Record `tid` as the active GPU driver.  Overwrites any previous registration.
pub fn register_gpu_driver(tid: usize) {
    GPU_DRIVER_CELL.store(tid, Ordering::Release);
    log::info!("[driver_cell] GPU driver registered: tid={}", tid);
}

/// Clear the GPU driver registration (called on cell exit/kill).
pub fn deregister_gpu_driver(tid: usize) {
    GPU_DRIVER_CELL
        .compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed)
        .ok();
}

/// TID of the registered input service Cell (0 = unregistered).
/// Set by the loader after spawning `/bin/input`; cleared on its death.
pub static INPUT_CELL_TID: AtomicUsize = AtomicUsize::new(0);

/// Register the input service cell.  Called by the loader after spawning `/bin/input`.
///
/// Logged at `warn!`, not `info!`: `/bin/input` spawns after the kernel drops
/// its log level to Warn at end-of-early-boot, so an info line here is
/// suppressed. This is a one-time boot-integrity event (the kernel now trusts
/// this TID as the sole keyboard-event sink) — worth surfacing, and the marker
/// the input-registration integration test asserts on.
pub fn set_input_cell(tid: usize) {
    INPUT_CELL_TID.store(tid, Ordering::Release);
    log::warn!("[input] registered input service TID {}", tid);
}

/// Clear the input service registration if it matches `tid` (called on cell death).
pub fn clear_input_cell_if(tid: usize) {
    INPUT_CELL_TID
        .compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed)
        .ok();
}
