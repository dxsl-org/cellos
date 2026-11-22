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
#[cfg(test)]
mod read_run_tests;
mod requests;
mod router;
#[cfg(test)]
mod tests;

pub use self::discovery::Discovery;

use self::router::Router;
use abi::block_route::ViBlockRouteV1;
use ostd::prelude::Mutex;
#[cfg(target_os = "none")]
use ostd::syscall as sys;
use types::block_wire::{BlockOutcome, SECTOR_SIZE};

/// Which block path a request takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceRef {
    /// Entry `n` of the device table built at discovery.
    Device(u8),
    /// The kernel block path; used only when discovery found no routes.
    Kernel,
}

/// How one bounded request/reply exchange with a provider ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// Nothing was accepted: the provider refused or was already gone.
    Refused,
    /// The provider died after accepting; the request may have run.
    PeerGone,
    /// [`BLOCK_REPLY_TICKS`] ran out. `dispatched`: the provider had taken the
    /// request, so it may have run.
    Expired { dispatched: bool },
    /// The provider replied with this many bytes.
    Replied(usize),
}

/// How long one block request may take, in scheduler ticks: the drivers' own
/// 30 s device budget plus 5 s, so only a driver that overran its deadline (a
/// frozen one) is cut off. 35 s also stays below the VFS heartbeat. Init's
/// bootstrap window (`VFS_BOOTSTRAP_READY_TICKS`) covers one such bound, so the
/// two change together.
pub const BLOCK_REPLY_TICKS: u64 = 3_500;

/// How long a stalled device stays refused before one request may probe it
/// again: a driver that was only slow (a long admin command, a loaded host)
/// recovers, while a frozen one costs one bound per window, not per request.
pub const STALL_RETRY_TICKS: u64 = 6_000;

/// Kernel and IPC operations the router needs.
pub trait BlockSys {
    /// Route slot `index` (syscall 432); `None` when there is no route there.
    fn lookup_route(&mut self, index: u32) -> Option<ViBlockRouteV1>;
    /// One exact-operation exchange with `tid`, bounded by [`BLOCK_REPLY_TICKS`].
    /// The reply lands at the start of `reply`.
    fn call(&mut self, tid: usize, request: &[u8], reply: &mut [u8]) -> Call;
    /// Scheduler ticks since boot (the stall window's clock).
    fn now_ticks(&mut self) -> u64;
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
                self.log(&alloc::format!(
                    "[vfs] block route {index} lookup refused: {error:?}"
                ));
                None
            }
        }
    }
    fn call(&mut self, tid: usize, request: &[u8], reply: &mut [u8]) -> Call {
        use ostd::ipc::IpcError;
        // An exact operation: a late reply can never answer a later request,
        // and a frozen provider costs one bound, not the whole service.
        crate::liveness::keepalive();
        match ostd::ipc::call_bytes(tid, request, reply, BLOCK_REPLY_TICKS) {
            Ok(bytes) => Call::Replied(bytes.len()),
            Err(IpcError::PreDispatchTimeout) => Call::Expired { dispatched: false },
            Err(IpcError::Indeterminate) => Call::Expired { dispatched: true },
            Err(IpcError::PeerGone) => Call::PeerGone,
            Err(_) => Call::Refused,
        }
    }
    fn now_ticks(&mut self) -> u64 {
        sys::sys_get_scheduler_ticks().unwrap_or(0)
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
    fn call(&mut self, _tid: usize, _request: &[u8], _reply: &mut [u8]) -> Call {
        Call::Refused
    }
    fn now_ticks(&mut self) -> u64 {
        0
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
    while ostd::service::provider_tid(abi::syscall::service::VFS).is_none() {
        let waited = match (start, sys::sys_get_scheduler_ticks()) {
            (Some(start), Some(now)) => now.saturating_sub(start),
            _ => u64::MAX,
        };
        if waited >= VFS_REGISTRATION_WAIT_TICKS {
            ostd::io::println(
                "[vfs] not registered as VFS after 500 ticks; block devices will refuse requests",
            );
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
///
/// Up to [`types::block_wire::MAX_RUN_SECTORS`] sectors move per request.
pub fn blk_read_sectors(dev: DeviceRef, abs_lba: u64, count: u32, buf: &mut [u8]) -> bool {
    let Some(buf) = buf.get_mut(..(count as usize) * SECTOR_SIZE) else {
        return false;
    };
    ROUTER.lock().read_sectors(&mut Syscalls, dev, abs_lba, buf)
}

/// Write `count` 512-byte sectors starting at absolute LBA `abs_lba` of `dev` from `data`.
///
/// Stops at the first sector that does not complete and returns its outcome;
/// earlier sectors stay written.
pub fn blk_write_sectors(dev: DeviceRef, abs_lba: u64, count: u32, data: &[u8]) -> BlockOutcome {
    if data.len() < (count as usize) * SECTOR_SIZE {
        return BlockOutcome::NotIssued;
    }
    for (i, sector) in data
        .as_chunks::<SECTOR_SIZE>()
        .0
        .iter()
        .take(count as usize)
        .enumerate()
    {
        let outcome = blk_write(dev, abs_lba + i as u64, sector);
        if !outcome.is_completed() {
            return outcome;
        }
    }
    BlockOutcome::Completed
}
