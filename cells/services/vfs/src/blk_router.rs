//! Absolute-LBA block router for all disk-backed VFS backends, per device.
//!
//! Every storage device a block Driver Cell registered (syscall 416) has a
//! kernel route slot; the VFS reads the slots once at start (syscall 432) into
//! its device table, and each backend addresses its device by [`DeviceRef`].
//! Requests go to the device's provider over the DrvRequest IPC wire. This is
//! the single place that speaks that wire, so FAT (`block_stream`) and CellosFS
//! (`disk_cellosfs`) route identically.
//!
//! The kernel block path (`sys_blk_*`) serves only [`DeviceRef::Kernel`], the
//! disk of boards where no Driver Cell registers a route (kernel-resident MMC).

mod discovery;
mod requests;
mod router;
#[cfg(test)]
mod tests;

pub use self::discovery::Discovery;

use self::router::Router;
use api::block_route::ViBlockRouteV1;
use ostd::prelude::Mutex;
#[cfg(target_os = "none")]
use ostd::syscall::{self as sys, SyscallResult};
use types::block_wire::{BlockOutcome, SECTOR_SIZE};

/// Which block path a request takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceRef {
    /// Entry `n` of the device table built at discovery.
    Device(u8),
    /// The kernel block path; used only when discovery found no routes.
    Kernel,
}

/// Kernel and IPC operations the router needs.
pub trait BlockSys {
    /// Route slot `index` (syscall 432); `None` when there is no route there.
    fn lookup_route(&mut self, index: u32) -> Option<ViBlockRouteV1>;
    /// `true` when the kernel accepted the message for `tid`.
    fn send(&mut self, tid: usize, msg: &[u8]) -> bool;
    /// `true` when a message from `tid` arrived in `buf`.
    fn recv(&mut self, tid: usize, buf: &mut [u8]) -> bool;
    fn kernel_read(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> bool;
    fn kernel_write(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> bool;
    fn kernel_flush(&mut self) -> bool;
    fn log(&mut self, line: &str);
}

/// The VFS block-device table; every block request is serialized through it.
static ROUTER: Mutex<Router> = Mutex::new(Router::new());

/// Scheduler ticks the VFS waits for its own `service::VFS` registration, which
/// the drivers require of every block request sender.
#[cfg(target_os = "none")]
const VFS_REGISTRATION_WAIT_TICKS: u64 = 500;

/// The live kernel behind [`BlockSys`].
struct Syscalls;

#[cfg(target_os = "none")]
impl BlockSys for Syscalls {
    fn lookup_route(&mut self, index: u32) -> Option<ViBlockRouteV1> {
        match sys::sys_lookup_block_route(index) {
            Ok(record) => record,
            Err(error) => {
                self.log(&alloc::format!("[vfs] block route {index} lookup refused: {error:?}"));
                None
            }
        }
    }
    fn send(&mut self, tid: usize, msg: &[u8]) -> bool {
        !matches!(sys::sys_send(tid, msg), SyscallResult::Err(_))
    }
    fn recv(&mut self, tid: usize, buf: &mut [u8]) -> bool {
        // Masked to `tid`, so no other cell's message is taken for the reply.
        matches!(sys::sys_recv(tid, buf), SyscallResult::Ok(from) if from == tid)
    }
    fn kernel_read(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> bool {
        sys::sys_blk_read(lba, buf)
    }
    fn kernel_write(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> bool {
        sys::sys_blk_write(lba, data)
    }
    fn kernel_flush(&mut self) -> bool {
        sys::sys_blk_flush()
    }
    fn log(&mut self, line: &str) {
        ostd::io::println(line);
    }
}

/// Hosted (test) builds have no kernel: no routes, and the kernel disk is absent.
#[cfg(not(target_os = "none"))]
impl BlockSys for Syscalls {
    fn lookup_route(&mut self, _index: u32) -> Option<ViBlockRouteV1> {
        None
    }
    fn send(&mut self, _tid: usize, _msg: &[u8]) -> bool {
        false
    }
    fn recv(&mut self, _tid: usize, _buf: &mut [u8]) -> bool {
        false
    }
    fn kernel_read(&mut self, _lba: u64, _buf: &mut [u8; SECTOR_SIZE]) -> bool {
        false
    }
    fn kernel_write(&mut self, _lba: u64, _data: &[u8; SECTOR_SIZE]) -> bool {
        false
    }
    fn kernel_flush(&mut self) -> bool {
        false
    }
    fn log(&mut self, _line: &str) {}
}

/// Wait (bounded) until this VFS is the registered `service::VFS`, then build
/// the device table from the kernel route slots and choose the primary disk.
pub fn discover() -> Discovery {
    wait_for_vfs_registration();
    discovery::discover(&mut ROUTER.lock(), &mut Syscalls)
}

/// Drivers refuse block requests from anyone but the registered VFS, and init
/// registers a freshly spawned VFS only after the spawn returns.
#[cfg(target_os = "none")]
fn wait_for_vfs_registration() {
    let start = sys::sys_get_scheduler_ticks();
    while sys::sys_lookup_service(api::syscall::service::VFS).is_none() {
        let waited = match (start, sys::sys_get_scheduler_ticks()) {
            (Some(start), Some(now)) => now.saturating_sub(start),
            _ => u64::MAX,
        };
        if waited >= VFS_REGISTRATION_WAIT_TICKS {
            ostd::io::println("[vfs] not registered as VFS after 500 ticks; block devices will refuse requests");
            return;
        }
        ostd::task::yield_now();
    }
}

#[cfg(not(target_os = "none"))]
fn wait_for_vfs_registration() {}

/// Read one 512-byte sector at absolute LBA `abs_lba` of `dev`.
pub fn blk_read(dev: DeviceRef, abs_lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> bool {
    ROUTER.lock().read(&mut Syscalls, dev, abs_lba, buf)
}

/// Write one 512-byte sector at absolute LBA `abs_lba` of `dev`.
///
/// `NotIssued`: nothing reached a device (send refused, or no live provider).
/// `Indeterminate`: the write may have been issued; the caller must not replay it.
pub fn blk_write(dev: DeviceRef, abs_lba: u64, data: &[u8; SECTOR_SIZE]) -> BlockOutcome {
    ROUTER.lock().write(&mut Syscalls, dev, abs_lba, data)
}

/// Flush `dev` only. Outcome rules as [`blk_write`].
pub fn blk_flush(dev: DeviceRef) -> BlockOutcome {
    ROUTER.lock().flush(&mut Syscalls, dev)
}

/// Read `count` 512-byte sectors starting at absolute LBA `abs_lba` of `dev` into `buf`.
pub fn blk_read_sectors(dev: DeviceRef, abs_lba: u64, count: u32, buf: &mut [u8]) -> bool {
    if buf.len() < (count as usize) * SECTOR_SIZE {
        return false;
    }
    for (i, chunk) in buf.chunks_exact_mut(SECTOR_SIZE).take(count as usize).enumerate() {
        let sector: &mut [u8; SECTOR_SIZE] = chunk.try_into().expect("whole sector chunk");
        if !blk_read(dev, abs_lba + i as u64, sector) {
            return false;
        }
    }
    true
}

/// Write `count` 512-byte sectors starting at absolute LBA `abs_lba` of `dev` from `data`.
///
/// Stops at the first sector that does not complete and returns its outcome;
/// earlier sectors stay written.
pub fn blk_write_sectors(dev: DeviceRef, abs_lba: u64, count: u32, data: &[u8]) -> BlockOutcome {
    if data.len() < (count as usize) * SECTOR_SIZE {
        return BlockOutcome::NotIssued;
    }
    for (i, chunk) in data.chunks_exact(SECTOR_SIZE).take(count as usize).enumerate() {
        let sector: &[u8; SECTOR_SIZE] = chunk.try_into().expect("whole sector chunk");
        let outcome = blk_write(dev, abs_lba + i as u64, sector);
        if !outcome.is_completed() {
            return outcome;
        }
    }
    BlockOutcome::Completed
}
