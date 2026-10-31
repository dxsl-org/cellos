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
use types::block_wire::{self, BlockOutcome, SECTOR_SIZE};

/// Sentinel: block Driver Cell not yet probed.
const NOT_PROBED: usize = 0;
/// Sentinel: probed and absent (no Driver Cell registered — use the kernel path).
const ABSENT: usize = usize::MAX;

/// Cached block Driver Cell TID. `NOT_PROBED` (0) until the first lookup.
static DRIVER_TID: AtomicUsize = AtomicUsize::new(NOT_PROBED);

/// Returns the block Driver Cell TID if one has registered, else `None`.
/// Caches a live driver TID across requests, re-checking service discovery
/// when no driver was registered previously.
fn driver_tid() -> Option<usize> {
    let cached = DRIVER_TID.load(Ordering::Relaxed);
    if cached != NOT_PROBED && cached != ABSENT {
        return Some(cached);
    }

    match sys_lookup_service(api::syscall::service::BLOCK_DRIVER) {
        Some(tid) if tid != 0 => {
            DRIVER_TID.store(tid, Ordering::Relaxed);
            Some(tid)
        }
        _ => None,
    }
}

/// Drop the cached driver TID after a failed exchange so the next request
/// re-probes service discovery.
///
/// A block driver cell that dies and is restarted registers under a new tid;
/// keeping the old one cached means every later request in that boot is sent to
/// a cell that no longer answers. `compare_exchange` leaves a tid cached by
/// another caller alone.
fn forget_driver(tid: usize) {
    let _ = DRIVER_TID.compare_exchange(tid, NOT_PROBED, Ordering::Relaxed, Ordering::Relaxed);
}

/// Read one 512-byte sector at absolute LBA `abs_lba`.
///
/// Routes to the block Driver Cell (DrvRequest IPC) when registered, falling
/// back to the kernel block path (`sys_blk_read`) for MMC-backed real boards.
pub fn blk_read(abs_lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> bool {
    if let Some(tid) = driver_tid() {
        // Read request: [op (2B)] [sector (8B)] — 10 bytes.
        let mut req = [0u8; 10];
        req[0..2].copy_from_slice(&block_wire::OP_READ.to_le_bytes());
        req[2..10].copy_from_slice(&abs_lba.to_le_bytes());
        if let SyscallResult::Err(_) = sys_send(tid, &req) {
            forget_driver(tid);
            return false;
        }
        // Reply: [status (1B)] [data (512B)] = 513 bytes. mask=tid so a message
        // from another cell (while we are blocked here) is never mistaken for it.
        let mut reply = [0u8; 1 + SECTOR_SIZE];
        // This recv result used to be dropped, and dropping it is not harmless:
        // the kernel delivers a driver cell's death as `Ok(dead_tid)` with the
        // reason written into the first 8 bytes of the buffer, so a cell that
        // died while this recv was parked left `reply[0] == 0` plus 512 bytes of
        // zeros - a successful read of data that was never on the device, from
        // which the filesystem then walked a FAT chain of zeros. Only `Ok(tid)`
        // is a reply; anything else is a failure the caller's retry re-probes.
        match sys_recv(tid, &mut reply) {
            SyscallResult::Ok(from) if from == tid => {}
            _ => {
                forget_driver(tid);
                return false;
            }
        }
        if !BlockOutcome::from_byte(reply[0]).is_completed() {
            return false;
        }
        buf.copy_from_slice(&reply[1..1 + SECTOR_SIZE]);
        true
    } else {
        sys_blk_read(abs_lba, buf)
    }
}

/// Write one 512-byte sector at absolute LBA `abs_lba`.
///
/// `Indeterminate` means the driver may have issued the write: the caller must
/// not replay it. A send the kernel refused never reached the driver
/// (`NotIssued`); a send followed by a missing reply (driver died while the
/// request was parked) may have been issued. The kernel fallback path reports
/// only success or failure, so its failure is `Indeterminate`.
pub fn blk_write(abs_lba: u64, data: &[u8; SECTOR_SIZE]) -> BlockOutcome {
    let outcome = if let Some(tid) = driver_tid() {
        // Write request: [op (2B)] [sector (8B)] [data (512B)] = 522 bytes.
        let mut req = [0u8; 10 + SECTOR_SIZE];
        req[0..2].copy_from_slice(&block_wire::OP_WRITE.to_le_bytes());
        req[2..10].copy_from_slice(&abs_lba.to_le_bytes());
        req[10..10 + SECTOR_SIZE].copy_from_slice(data);
        exchange_status(tid, &req)
    } else if sys_blk_write(abs_lba, data) {
        BlockOutcome::Completed
    } else {
        BlockOutcome::Indeterminate
    };
    if outcome == BlockOutcome::Indeterminate {
        ostd::io::println(&alloc::format!(
            "[blk-router] write lba={} indeterminate; not replayed",
            abs_lba
        ));
    }
    outcome
}

/// Flush all dirty data for the block device. Outcome rules as [`blk_write`].
pub fn blk_flush() -> BlockOutcome {
    let outcome = if let Some(tid) = driver_tid() {
        exchange_status(tid, &block_wire::OP_FLUSH.to_le_bytes())
    } else if ostd::syscall::sys_blk_flush() {
        BlockOutcome::Completed
    } else {
        BlockOutcome::Indeterminate
    };
    if !outcome.is_completed() {
        ostd::io::println(&alloc::format!("[blk-router] flush {:?}", outcome));
    }
    outcome
}

/// Send a write/flush request and decode its one-byte status reply.
fn exchange_status(tid: usize, req: &[u8]) -> BlockOutcome {
    if let SyscallResult::Err(_) = sys_send(tid, req) {
        forget_driver(tid);
        return BlockOutcome::NotIssued;
    }
    let mut reply = [0u8; 1];
    // Only `Ok(tid)` is a reply: the kernel delivers a driver cell's death as
    // `Ok(dead_tid)`, and an unanswered write read as success would make the
    // write-through page cache believe a sector reached the device.
    match sys_recv(tid, &mut reply) {
        SyscallResult::Ok(from) if from == tid => BlockOutcome::from_byte(reply[0]),
        other => {
            ostd::io::println(&alloc::format!(
                "[blk-router] driver reply missing after send: {:?}",
                other
            ));
            forget_driver(tid);
            BlockOutcome::Indeterminate
        }
    }
}

/// Read `count` 512-byte sectors starting at absolute LBA `abs_lba` into `buf`.
pub fn blk_read_sectors(abs_lba: u64, count: u32, buf: &mut [u8]) -> bool {
    if buf.len() < (count as usize) * SECTOR_SIZE {
        return false;
    }
    for i in 0..count {
        let mut tmp = [0u8; SECTOR_SIZE];
        if !blk_read(abs_lba + i as u64, &mut tmp) {
            return false;
        }
        let off = (i as usize) * SECTOR_SIZE;
        buf[off..off + SECTOR_SIZE].copy_from_slice(&tmp);
    }
    true
}

/// Write `count` 512-byte sectors starting at absolute LBA `abs_lba` from `data`.
///
/// Stops at the first sector that does not complete and returns its outcome;
/// earlier sectors stay written.
pub fn blk_write_sectors(abs_lba: u64, count: u32, data: &[u8]) -> BlockOutcome {
    if data.len() < (count as usize) * SECTOR_SIZE {
        return BlockOutcome::NotIssued;
    }
    for i in 0..count {
        let off = (i as usize) * SECTOR_SIZE;
        let mut tmp = [0u8; SECTOR_SIZE];
        tmp.copy_from_slice(&data[off..off + SECTOR_SIZE]);
        let outcome = blk_write(abs_lba + i as u64, &tmp);
        if !outcome.is_completed() {
            return outcome;
        }
    }
    BlockOutcome::Completed
}
