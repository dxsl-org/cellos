#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
// F1 (Spec 16 §6): grant copies go through ostd's leased safe adapters, so the
// VFS cell holds no `unsafe` of its own.
#![forbid(unsafe_code)]

extern crate alloc;
extern crate driver_disk;

// Host test harness: this is a bare-metal binary target, so a hosted build
// must not compile the cell entry. Everything image-shaped is gated on the
// *target*, not on `cfg(test)` — `service-net` is the module that got this
// right first: a `no_std`-only gate left `#[no_mangle] pub fn main` in the
// test binary, where the C runtime called it instead of libtest and the
// harness hung before it could list a single one of the tests below.

mod access;
mod backend;
mod backend_bin_overlay;
mod backend_bootfs;
mod backend_cellosfs;
mod backend_fat;
mod backend_ramfs;
mod blk_router;
mod block_stream;
mod caller;
mod dir_admission;
mod dir_ops;
#[cfg(test)]
mod dir_ops_tests;
mod dirs;
mod disk_cellosfs;
mod dispatch;
mod dispatch_dirs;
mod dispatch_file_handles;
mod dispatch_grants;
mod dispatch_paths;
mod fat_extent_map;
mod file_handles;
mod fs_error;
mod grant_read;
mod grant_write;
#[cfg(target_os = "none")]
mod liveness;
mod manager;
mod mount;
mod namespace;
mod page_cache;
mod paths;
mod pending;
mod prepared_cwd;
mod quota;
mod save;
mod subtree;

use manager::VfsManager;
use ostd::io::println;

#[cfg(target_os = "none")]
static GLOBAL_VFS: ostd::prelude::Mutex<Option<VfsManager>> = ostd::prelude::Mutex::new(None);

// Declares block-I/O capability; the kernel grants BlockIoCap at spawn.
// part_data/part_lfs scope the raw block syscalls to P1 (FAT32) + P4
// (littlefs) — P2 cell-table and P3 snapshot stay kernel-only (P03 design).
api::declare_manifest!(
    block_io = true,
    network = false,
    spawn = false,
    part_data = true,
    part_lfs = true
);

// Narrow syscall allowlist — kernel enforces this at dispatch (Phase 27).
// BootFS proxy (/bin via the kernel initramfs VIFS1): Open/Close/ReadDir for
// listing (all synchronous), OpenCap/ReadCap/CloseCap for file reads — the FD
// `Read` syscall is deliberately ABSENT: it is an async transformation that
// requires the caller to park immediately, which a service dispatch loop
// cannot do (see backend_bootfs.rs::read_to_vec).
api::declare_syscalls![
    Send,
    SendGather,
    Recv,
    TryRecv,
    Log,
    Heartbeat,
    GrantAlloc,
    GrantShare,
    GrantSlice,
    GrantFree,
    BlkReadAsync,
    GrantRegister,
    GrantUnregister,
    StateStash,
    StateRestore,
    Open,
    Close,
    ReadDir,
    OpenCap,
    ReadCap,
    // SeekCap shares ReadCap's allowlist bit (bit 16), so positional reads add no authority; it
    // is the difference between a chunked read costing O(file) and O(file^2).
    SeekCap,
    CloseCap,
    // Prepared-cwd commits (`prepared_cwd`): the own service binding (LookupServiceBound)
    // and the kernel SessionV1 requests (`ostd::session::kernel_request`; CellSessionV1 has
    // no allowlist bit, it is listed for the record). The bounded exact-operation block
    // exchange (`blk_router`: IpcSubmit/IpcTake/IpcWait/IpcCancel share Send/Recv's bits)
    // and its scheduler-tick budget and the grant-hold clock (GetTime).
    LookupServiceBound,
    IpcSubmit,
    IpcTake,
    IpcWait,
    IpcCancel,
    GetTime,
    CellSessionV1,
    // NOTE: deliberately NO SetTimer. VFS never calls it — a "SetTimer (bit 11)
    // denied for tid <vfs>" kernel warn on x86 is the CANARY for the known x86
    // syscall-redispatch corruption (syscall number read as user CS = 0x23 = 35
    // with a pointer as the tick count; see TODO #9 / x86 q35 P02). Allowing it
    // turns that corruption into an unbounded sleep that hangs the boot.
];

// `ostd::heap` (and therefore the macro) exists only for the bare-metal target;
// the host test build has no image-provided heap to declare.
//
// The size is named because the sector cache in `page_cache` has to fit inside
// it: the cache declares its budget as a fraction of this, and a cache that
// cannot fit kills the cell as soon as it fills (`OOM: cell heap exhausted`).
pub(crate) const VFS_HEAP_BYTES: usize = 16 * 1024 * 1024;

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(VFS_HEAP_BYTES);

// The macro's expansion carries the loader's `#[no_mangle]`, so this crate can
// stay under `forbid(unsafe_code)`.
#[cfg(target_os = "none")]
ostd::cell_main!(vfs_main);

#[cfg(target_os = "none")]
fn vfs_main() {
    init_custom_heap();
    println("VFS Service v0.2: RamFS + mkdir/rmdir/unlink IPC (typed postcard)");
    // VfsManager::new() mounts all backends, including the FAT volume on the
    // VirtIO disk (which logs its own success/fallback status).
    let vfs = VfsManager::new();
    // Whether the idle reconcile cadence still has kernel state to poll; see the loop.
    let idle_work = prepared_cwd::idle_work(&vfs);
    #[cfg(feature = "test-hooks")]
    {
        file_handles::selftest::run();
        access::selftest::run();
        manager::read_path_selftest::run();
        save::selftest::run();
    }
    *GLOBAL_VFS.lock() = Some(vfs);
    liveness::arm();
    let config = ostd::service::ServeConfig {
        heartbeat_ticks: liveness::VFS_HEARTBEAT_TICKS,
        ..ostd::service::ServeConfig::with_heartbeat()
    }
    .watching(ostd::service::WatchPolicy::Every, MAX_WATCHED_OWNERS);
    ostd::service::serve(
        config,
        &mut Vfs {
            idle_work,
            idle_turns: 0,
        },
    );
}

/// Owners VFS watches at once: every live Cell that holds VFS state. A caller
/// beyond it is refused, never served unwatched.
#[cfg(target_os = "none")]
const MAX_WATCHED_OWNERS: usize = 256;

/// The request loop's own state; the file system lives in `GLOBAL_VFS`.
#[cfg(target_os = "none")]
struct Vfs {
    /// Whether bounded preparations are still unsettled in the kernel. They can
    /// change without a message (they expire, commit or end), so only then does
    /// the loop poll and reconcile; otherwise every change arrives as a request
    /// or an owner death and the loop parks. Recomputed wherever state changes.
    idle_work: bool,
    idle_turns: u32,
}

#[cfg(target_os = "none")]
impl ostd::service::Handler for Vfs {
    fn on_request(&mut self, request: ostd::service::Request<'_>) {
        let caller = caller::Caller::from_attested(*request.caller());
        let mut encoded = [0u8; api::ipc::IPC_BUF_SIZE];
        let encoded_len = {
            let mut resp_buf = [0u8; api::ipc::IPC_BUF_SIZE];
            let mut guard = GLOBAL_VFS.lock();
            let vfs = guard
                .as_mut()
                .expect("VFS initialized before serving requests");
            vfs.note_watched_owner(caller);
            let response =
                dispatch::handle_request(vfs, request.payload(), Some(caller), &mut resp_buf);
            self.idle_work = prepared_cwd::idle_work(vfs);
            api::ipc::encode(&response, &mut encoded)
                .map(|bytes| bytes.len())
                .unwrap_or(0)
        };
        // `GLOBAL_VFS` is unlocked before replying to the caller.
        let _ = request.reply(&encoded[..encoded_len]);
    }

    /// No owner watch, no durable state: the request is refused.
    fn on_unwatched(&mut self, request: ostd::service::Request<'_>) {
        let mut encoded = [0u8; api::ipc::IPC_BUF_SIZE];
        let denied = api::ipc::VfsResponse::Err(api::services::proto::vfs::VfsError::Denied as u8);
        let len = api::ipc::encode(&denied, &mut encoded)
            .map(|bytes| bytes.len())
            .unwrap_or(0);
        let _ = request.reply(&encoded[..len]);
    }

    fn on_owner_death(&mut self, owner: ostd::service::Owner) {
        if let Some(vfs) = GLOBAL_VFS.lock().as_mut() {
            let _ = vfs.owner_died(owner.cell_id, owner.generation);
            prepared_cwd::reconcile(vfs);
            self.idle_work = prepared_cwd::idle_work(vfs);
        }
    }

    fn on_idle(&mut self) -> ostd::service::Wait {
        // VFS holds no SetTimer authority (see the allowlist), so the reconcile
        // cadence counts polling turns instead of wall time; requests reconcile
        // only the caller's own reservations (`dir_admission::admit`).
        const IDLE_RECONCILE_EVERY: u32 = 128;
        if !self.idle_work {
            return ostd::service::Wait::Block;
        }
        self.idle_turns = self.idle_turns.wrapping_add(1);
        if self.idle_turns.is_multiple_of(IDLE_RECONCILE_EVERY) {
            if let Some(vfs) = GLOBAL_VFS.lock().as_mut() {
                prepared_cwd::reconcile(vfs);
                self.idle_work = prepared_cwd::idle_work(vfs);
            }
        }
        ostd::service::Wait::Poll
    }
}
