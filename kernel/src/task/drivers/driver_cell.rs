//! Driver Cell registration — tracks which cells own the block and NIC roles.
//!
//! When a Tier-1 Driver Cell calls `sys_register_block_driver` or
//! `sys_register_nic_driver`, the kernel records its TID here.  Service clients
//! (VFS, net cell) use `sys_lookup_service(service::BLOCK_DRIVER)` to find the
//! provider; these statics are the backing store for that lookup.
//!
//! `0` means "no driver cell registered; fall back to kernel-resident driver".

use core::sync::atomic::{AtomicUsize, Ordering};

/// TID of the registered block Driver Cell (0 = none; kernel NVMe/VirtIO/MMC is active).
pub static BLOCK_DRIVER_CELL: AtomicUsize = AtomicUsize::new(0);

/// TID of the registered NIC Driver Cell (0 = none; kernel e1000/VirtIO is active).
pub static NIC_DRIVER_CELL: AtomicUsize = AtomicUsize::new(0);

/// Record `tid` as the active block driver.  Overwrites any previous registration.
pub fn register_block_driver(tid: usize) {
    BLOCK_DRIVER_CELL.store(tid, Ordering::Release);
    log::info!("[driver_cell] block driver registered: tid={}", tid);
}

/// Record `tid` as the active NIC driver.  Overwrites any previous registration.
pub fn register_nic_driver(tid: usize) {
    NIC_DRIVER_CELL.store(tid, Ordering::Release);
    log::info!("[driver_cell] NIC driver registered: tid={}", tid);
}

/// Clear the block driver registration (called on cell exit/kill).
pub fn deregister_block_driver(tid: usize) {
    BLOCK_DRIVER_CELL.compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed).ok();
}

/// Clear the NIC driver registration (called on cell exit/kill).
pub fn deregister_nic_driver(tid: usize) {
    NIC_DRIVER_CELL.compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed).ok();
}
