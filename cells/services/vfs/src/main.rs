#![no_std]
#![no_main]

extern crate alloc;
extern crate driver_disk;
// redox_syscall's [lib] name is "syscall"; alias so our code can use redox_syscall:: paths.
extern crate syscall as redox_syscall;

mod access;
mod backend;
mod backend_bin_overlay;
mod backend_bootfs;
mod backend_fat;
#[cfg(feature = "littlefs")]
mod backend_littlefs;
mod backend_ramfs;
mod backend_redoxfs;
mod blk_router;
mod block_stream;
mod disk_redoxfs;
#[cfg(feature = "littlefs")]
mod lfs_disk;
// x86_64-only str* providers for the littlefs C core — the api POSIX shim
// (which provides them on riscv64/aarch64) is cfg-gated off on x86_64 to
// avoid duplicate symbols with mlibc Tier-B cells.
mod caller;
mod dir_admission;
mod dirs;
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
#[cfg(all(feature = "littlefs", target_arch = "x86_64"))]
mod lfs_string_shim;
mod manager;
mod namespace;
mod mount;
mod page_cache;
mod paths;
mod pending;
mod quota;
mod subtree;

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
    CloseCap,
    // NOTE: deliberately NO SetTimer. VFS never calls it — a "SetTimer (bit 11)
    // denied for tid <vfs>" kernel warn on x86 is the CANARY for the known x86
    // syscall-redispatch corruption (syscall number read as user CS = 0x23 = 35
    // with a pointer as the tick count; see TODO #9 / x86 q35 P02). Allowing it
    // turns that corruption into an unbounded sleep that hangs the boot.
];

#[no_mangle]
pub fn main() {
    println("VFS Service v0.2: RamFS + mkdir/rmdir/unlink IPC (typed postcard)");
    // VfsManager::new() mounts all backends, including the FAT volume on the
    // VirtIO disk (which logs its own success/fallback status).
    let vfs = VfsManager::new();
    #[cfg(feature = "test-hooks")]
    {
        file_handles::selftest::run();
        access::selftest::run();
    }
    *GLOBAL_VFS.lock() = Some(vfs);

    // Register the fast-IPC handler so trusted Cells can bypass ecall for VFS reads.
    // The kernel records the VFS cell's ID at spawn time so it can clear this
    // pointer if VFS crashes — see loader.rs fast_ipc::set_vfs_handler_cell call.
    ostd::fast_ipc::register_vfs(vfs_fast_handler);
    let mut buf = [0u8; api::ipc::IPC_BUF_SIZE];

    loop {
        // `sys_recv_attested` asks the kernel to state which cell sent the
        // message, in the tail of `buf`.  VFS cannot derive that itself: `sender`
        // is a tid, and a thread has its own tid while belonging to its parent
        // cell, so a cell id built from that tid named a cell that does not exist
        // and charged its quota to a ledger row nothing owned.
        match ostd::syscall::sys_recv_attested(0, &mut buf) {
            ostd::syscall::SyscallResult::Ok(sender) if sender > 0 => {
                let Some(identity) = api::caller_identity::CallerIdentity::from_recv_buf(&buf)
                else {
                    let cancellations = GLOBAL_VFS
                        .lock()
                        .as_mut()
                        .map(|vfs| {
                            let _ = vfs.handle_unattributed_owner_death(sender);
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
                    ostd::syscall::sys_send(sender, &encoded[..len]);
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
                    (len, vfs.take_owner_watch_cancellations())
                };
                for cancelled in cancellations {
                    ostd::syscall::sys_cancel_cell_owner_watch(cancelled);
                }
                // `GLOBAL_VFS` is unlocked for every kernel call and send.
                ostd::syscall::sys_send(sender, &encoded[..encoded_len]);
                buf = [0u8; api::ipc::IPC_BUF_SIZE];
            }
            _ => ostd::task::yield_now(),
        }
    }
}
