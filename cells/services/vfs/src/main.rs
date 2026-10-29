#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

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
mod dirs;
mod disk_cellosfs;
mod dispatch;
mod dispatch_dirs;
mod dispatch_file_handles;
mod dispatch_grants;
mod dispatch_paths;
mod fast_handler;
mod file_handles;
mod grant_read;
mod grant_write;
mod handle_table;
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
mod trust;

use fast_handler::{vfs_fast_handler, GLOBAL_VFS};
use manager::VfsManager;
use ostd::io::println;

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
    Recv,
    TryRecv,
    Reply,
    Log,
    Heartbeat,
    LookupService,
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
    // First-trust confirmation (S9-4b): VFS calls service-input over the authenticated
    // SessionV1 transport (`ostd::session::register_trust_challenge` /
    // `collect_trust_decision`): the issuer binding (LookupServiceBound shares
    // LookupService's bit), the bounded IPC exchange (IpcSubmit/IpcTake/IpcWait/IpcCancel
    // share Send/Recv's bits), its scheduler-tick budget and the monotonic challenge
    // deadline (GetTime). CellSessionV1 has no allowlist bit; it is listed for the record.
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

#[no_mangle]
#[cfg(target_os = "none")]
pub fn main() {
    init_custom_heap();
    println("VFS Service v0.2: RamFS + mkdir/rmdir/unlink IPC (typed postcard)");
    // VfsManager::new() mounts all backends, including the FAT volume on the
    // VirtIO disk (which logs its own success/fallback status).
    let vfs = VfsManager::new();
    // Whether the idle reconcile cadence still has kernel state to poll; see the loop.
    let mut idle_work = prepared_cwd::idle_work(&vfs);
    #[cfg(feature = "test-hooks")]
    {
        file_handles::selftest::run();
        access::selftest::run();
        manager::read_path_selftest::run();
        save::selftest::run();
    }
    *GLOBAL_VFS.lock() = Some(vfs);

    // Register the fast-IPC handler so trusted Cells can bypass ecall for VFS reads.
    // The kernel records the VFS cell's ID at spawn time so it can clear this
    // pointer if VFS crashes — see loader.rs fast_ipc::set_vfs_handler_cell call.
    ostd::fast_ipc::register_vfs(vfs_fast_handler);
    let mut buf = [0u8; api::ipc::IPC_BUF_SIZE];
    // Full reconciliation runs once per this many idle turns. VFS holds no SetTimer
    // authority (deliberately withheld, see the allowlist; GetTime is only the trust-call
    // budget), so the cadence counts idle receive attempts instead of wall time; requests
    // reconcile only the caller's own reservations (`dir_admission::admit`).
    const IDLE_RECONCILE_EVERY: u32 = 128;
    let mut idle_turns: u32 = 0;

    loop {
        // The kernel writes caller identity in the receive-buffer trailer.
        // A sender TID is reply routing only, never a Cell principal.
        //
        // Only bounded preparations can change without a message (they expire, commit
        // or end in the kernel), so the nonblocking receive and its reconcile cadence run
        // only while `prepared_cwd::idle_work` says one is unsettled. Otherwise every
        // change arrives as a request or an owner-death notification, and the loop
        // parks in the blocking receive instead of yield-spinning. `idle_work` is
        // recomputed wherever the loop changes VFS state; the fast-IPC handler only reads.
        let received = if idle_work {
            ostd::syscall::sys_try_recv_attested(0, &mut buf)
        } else {
            ostd::syscall::sys_recv_attested(0, &mut buf)
        };
        match received {
            ostd::syscall::SyscallResult::Ok(sender) if sender > 0 => {
                let operation = ostd::ipc::current();
                let Some(identity) = api::caller_identity::CallerIdentity::from_recv_buf(&buf)
                else {
                    let cancellations = GLOBAL_VFS
                        .lock()
                        .as_mut()
                        .map(|vfs| {
                            let _ = vfs.handle_unattributed_owner_death(sender);
                            prepared_cwd::reconcile(vfs);
                            idle_work = prepared_cwd::idle_work(vfs);
                            vfs.take_owner_watch_cancellations()
                        })
                        .unwrap_or_default();
                    for token in cancellations {
                        ostd::syscall::sys_cancel_cell_owner_watch(token);
                    }
                    buf = [0u8; api::ipc::IPC_BUF_SIZE];
                    continue;
                };
                let caller = caller::Caller::from_attested(identity);
                // The atomic kernel operation verifies this exact receive context
                // and registers root death before VFS can persist any state.
                let Some((owner, token)) =
                    ostd::syscall::sys_watch_cell_owner(caller.cell.0, caller.generation)
                else {
                    let mut encoded = [0u8; api::ipc::IPC_BUF_SIZE];
                    let len = api::ipc::encode(&api::ipc::VfsResponse::Err(3), &mut encoded)
                        .map(|bytes| bytes.len())
                        .unwrap_or(0);
                    if let Some(operation) = operation {
                        let _ = ostd::ipc::reply(operation, &encoded[..len]);
                    } else {
                        ostd::syscall::sys_send(sender, &encoded[..len]);
                    }
                    buf = [0u8; api::ipc::IPC_BUF_SIZE];
                    continue;
                };
                let mut encoded = [0u8; api::ipc::IPC_BUF_SIZE];
                let (encoded_len, cancellations) = {
                    let mut resp_buf = [0u8; api::ipc::IPC_BUF_SIZE];
                    let mut guard = GLOBAL_VFS.lock();
                    let vfs = guard
                        .as_mut()
                        .expect("VFS initialized before serving requests");
                    vfs.install_owner_watch(caller, owner.root_tid as usize, token);
                    let response = dispatch::handle_request(vfs, &buf, Some(caller), &mut resp_buf);
                    let len = api::ipc::encode(&response, &mut encoded)
                        .map(|bytes| bytes.len())
                        .unwrap_or(0);
                    idle_work = prepared_cwd::idle_work(vfs);
                    (len, vfs.take_owner_watch_cancellations())
                };
                for cancelled in cancellations {
                    ostd::syscall::sys_cancel_cell_owner_watch(cancelled);
                }
                // `GLOBAL_VFS` is unlocked before replying to the caller.
                if let Some(operation) = operation {
                    let _ = ostd::ipc::reply(operation, &encoded[..encoded_len]);
                } else {
                    ostd::syscall::sys_send(sender, &encoded[..encoded_len]);
                }
                buf = [0u8; api::ipc::IPC_BUF_SIZE];
            }
            _ => {
                if idle_work {
                    idle_turns = idle_turns.wrapping_add(1);
                    if idle_turns % IDLE_RECONCILE_EVERY == 0 {
                        if let Some(vfs) = GLOBAL_VFS.lock().as_mut() {
                            prepared_cwd::reconcile(vfs);
                            idle_work = prepared_cwd::idle_work(vfs);
                        }
                    }
                }
                ostd::task::yield_now();
            }
        }
    }
}
