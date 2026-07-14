//! Shared absolute-LBA block router for all disk-backed VFS backends.
//!
//! Since the G2 loader redesign the kernel drives no block hardware on QEMU:
//! `sys_blk_read`/`sys_blk_write` reach only the kernel's null device. Sector
//! I/O must go to the registered block Driver Cell (`service::BLOCK_DRIVER`,
//! the virtio-blk / NVMe cell) via the DrvRequest IPC protocol. This module is
//! the single place that speaks that protocol so every backend (FAT via
//! `block_stream`, littlefs via `lfs_disk`) routes identically — before this was
//! extracted, littlefs still called the raw syscalls and silently hit the dead
//! null device, so every `/data` write failed.
//!
//! The kernel path remains the fallback for real boards where an MMC/SDHCI
//! block device is kernel-resident and no Driver Cell registers.

use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::syscall::{
    sys_blk_read, sys_blk_write, sys_lookup_service, sys_recv, sys_send, SyscallResult,
};

const SECTOR_SIZE: usize = 512;

/// Sentinel: block Driver Cell not yet probed.
const NOT_PROBED: usize = 0;
/// Sentinel: probed and absent (no Driver Cell registered — use the kernel path).
const ABSENT: usize = usize::MAX;

/// Cached block Driver Cell TID. `NOT_PROBED` (0) until the first lookup.
static DRIVER_TID: AtomicUsize = AtomicUsize::new(NOT_PROBED);

/// Returns the block Driver Cell TID if one has registered, else `None`.
/// Caches the result so only the first call performs the lookup syscall.
fn driver_tid() -> Option<usize> {
    let cached = DRIVER_TID.load(Ordering::Relaxed);
    if cached == ABSENT {
        return None;
    }
    if cached != NOT_PROBED {
        return Some(cached);
    }

    match sys_lookup_service(api::syscall::service::BLOCK_DRIVER) {
        Some(tid) if tid != 0 => {
            DRIVER_TID.store(tid, Ordering::Relaxed);
            Some(tid)
        }
        _ => {
            DRIVER_TID.store(ABSENT, Ordering::Relaxed);
            None
        }
    }
}

/// Read one 512-byte sector at absolute LBA `abs_lba`.
///
/// Routes to the block Driver Cell (DrvRequest IPC) when registered, falling
/// back to the kernel block path (`sys_blk_read`) for MMC-backed real boards.
pub fn blk_read(abs_lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> bool {
    if let Some(tid) = driver_tid() {
        // Read request: [op=0 (2B)] [sector (8B)] — 10 bytes.
        let mut req = [0u8; 10];
        req[0..2].copy_from_slice(&0u16.to_le_bytes());
        req[2..10].copy_from_slice(&abs_lba.to_le_bytes());
        if let SyscallResult::Err(_) = sys_send(tid, &req) {
            return false;
        }
        // Reply: [status (1B)] [data (512B)] = 513 bytes. mask=tid so a message
        // from another cell (while we are blocked here) is never mistaken for it.
        let mut reply = [0u8; 1 + SECTOR_SIZE];
        sys_recv(tid, &mut reply);
        if reply[0] != 0 {
            return false;
        }
        buf.copy_from_slice(&reply[1..1 + SECTOR_SIZE]);
        true
    } else {
        sys_blk_read(abs_lba, buf)
    }
}

/// Write one 512-byte sector at absolute LBA `abs_lba`.
pub fn blk_write(abs_lba: u64, data: &[u8; SECTOR_SIZE]) -> bool {
    if let Some(tid) = driver_tid() {
        // Write request: [op=1 (2B)] [sector (8B)] [data (512B)] = 522 bytes.
        let mut req = [0u8; 10 + SECTOR_SIZE];
        req[0..2].copy_from_slice(&1u16.to_le_bytes());
        req[2..10].copy_from_slice(&abs_lba.to_le_bytes());
        req[10..10 + SECTOR_SIZE].copy_from_slice(data);
        if let SyscallResult::Err(_) = sys_send(tid, &req) {
            return false;
        }
        let mut reply = [0u8; 1];
        sys_recv(tid, &mut reply);
        reply[0] == 0
    } else {
        sys_blk_write(abs_lba, data)
    }
}
