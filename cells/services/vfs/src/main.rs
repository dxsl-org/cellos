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
mod file_handles;
mod handle_table;
#[cfg(all(feature = "littlefs", target_arch = "x86_64"))]
mod lfs_string_shim;
mod manager;
mod mount;
mod page_cache;
mod paths;
mod pending;
mod quota;
mod subtree;

use manager::VfsManager;
use ostd::io::println;
use ostd::prelude::*;

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

// Global VFS manager for the fast-IPC handler (which runs outside the main recv loop).
// Protected by a spinlock; on single-hart there is no actual contention.
static GLOBAL_VFS: Mutex<Option<VfsManager>> = Mutex::new(None);

/// Fast-IPC handler: serves VfsRequest::GetFile without ecall overhead.
///
/// Authorized exactly like the ecall path.  It has to be: `GetFile` replies with a
/// raw `DataPtr`, which in a single address space is permanent read authority that
/// cannot be revoked once handed out — so an ungated fast path would make the gate
/// on the ecall path decorative.  `caller` comes from the kernel
/// (`kernel::fast_ipc::call_vfs` resolves it from live scheduler state), never from
/// an argument this cell's client controls; `None` means unattributable, which is
/// refused.
///
/// A cell this service has never served over the ecall path is declined with a
/// zero-length reply, which `call_vfs` callers treat as "fast path unavailable"
/// and retry as an ordinary syscall.  The reason is the seal: deciding whether a
/// cell may still name a path needs the kernel's provenance record, and pulling
/// that is a syscall this handler cannot make with interrupts disabled.  Serving
/// an unknown cell here would therefore serve a path read to a cell that should
/// already have been refused one.  The cost is a single ecall per cell.
///
/// # Safety
/// Called with S-mode interrupts disabled (guaranteed by `ostd::fast_ipc::call_vfs`).
unsafe fn vfs_fast_handler(
    caller: Option<api::caller_identity::CallerIdentity>,
    req: &api::ipc::VfsRequest<'_>,
    out: &mut [u8; api::ipc::IPC_BUF_SIZE],
) -> usize {
    let resp = match caller.map(crate::caller::Caller::from_attested) {
        None => api::ipc::VfsResponse::Err(3), // unknown caller → denied
        Some(caller) => match req {
            api::ipc::VfsRequest::GetFile(path) => {
                if let Some(vfs) = GLOBAL_VFS.lock().as_ref() {
                    if !vfs.dirs.has_met(caller) {
                        return 0; // decline; the ecall path will decide
                    }
                    // Sealed and unauthorized are one refusal: this cell may not
                    // read this path, and which rule said so is not the caller's
                    // business.
                    if vfs.dirs.is_sealed(caller) || !vfs.access.can_read(caller, path) {
                        api::ipc::VfsResponse::Err(3)
                    } else if let Some((ptr, len)) = vfs.get_file_ptr(path) {
                        api::ipc::VfsResponse::DataPtr {
                            ptr: ptr as u64,
                            len: len as u64,
                        }
                    } else {
                        api::ipc::VfsResponse::Err(1)
                    }
                } else {
                    api::ipc::VfsResponse::Err(0xFF)
                }
            }
            _ => api::ipc::VfsResponse::Err(0xFE), // other ops must use ecall path
        },
    };
    api::ipc::encode(&resp, out).map(|s| s.len()).unwrap_or(0)
}

#[no_mangle]
pub fn main() {
    println("VFS Service v0.2: RamFS + mkdir/rmdir/unlink IPC (typed postcard)");
    // VfsManager::new() mounts all backends, including the FAT volume on the
    // VirtIO disk (which logs its own success/fallback status).
    let vfs = VfsManager::new();
    #[cfg(feature = "test-hooks")]
    file_handles::selftest::run();
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
                if api::caller_identity::CallerIdentity::from_recv_buf(&buf).is_none() {
                    if let Some(vfs) = GLOBAL_VFS.lock().as_mut() {
                        let _ = vfs.handle_unattributed_owner_death(sender);
                    }
                    buf = [0u8; api::ipc::IPC_BUF_SIZE];
                    continue;
                }
                // Encode the response into a local buffer while holding the VFS lock,
                // then DROP the lock before sys_send.  If ipc_send blocks (client not
                // yet in Recv), yield_cpu switches to another cell.  That cell may call
                // call_vfs which also acquires GLOBAL_VFS — a deadlock if we still hold
                // the lock during the send.
                let mut encoded = [0u8; api::ipc::IPC_BUF_SIZE];
                let mut encoded_len: usize;
                let watch;
                {
                    let mut resp_buf = [0u8; api::ipc::IPC_BUF_SIZE];
                    // Acquire VFS state; released at end of this block, before sys_send.
                    let mut gvfs = GLOBAL_VFS.lock();
                    let vfs = gvfs
                        .as_mut()
                        .expect("VFS initialized before serving requests");
                    // `None` here (no trailer, or a sender the kernel could no
                    // longer attribute) makes every op deny — see handle_request.
                    let attested = api::caller_identity::CallerIdentity::from_recv_buf(&buf)
                        .map(caller::Caller::from_attested);
                    let resp = dispatch::handle_request(vfs, &buf, attested, &mut resp_buf);
                    watch = attested.and_then(|caller| {
                        vfs.should_watch_after_response(caller, &resp)
                            .map(|owner_tid| (owner_tid, caller))
                    });
                    // Encode while holding the lock (safe: no sys_send yet).
                    encoded_len = api::ipc::encode(&resp, &mut encoded)
                        .map(|s| s.len())
                        .unwrap_or(0);
                } // GLOBAL_VFS lock released here — before sys_send

                if let Some((owner_tid, caller)) = watch {
                    match ostd::syscall::sys_notify_on_exit(owner_tid) {
                        ostd::syscall::SyscallResult::Ok(_) => {}
                        _ => {
                            if let Some(vfs) = GLOBAL_VFS.lock().as_mut() {
                                vfs.rollback_owner_watch(caller);
                            }
                            encoded_len =
                                api::ipc::encode(&api::ipc::VfsResponse::Err(3), &mut encoded)
                                    .map(|s| s.len())
                                    .unwrap_or(0);
                        }
                    }
                }

                // Send after releasing the lock so a blocked ipc_send + yield_cpu
                // cannot switch to a cell that deadlocks on GLOBAL_VFS.
                ostd::syscall::sys_send(sender, &encoded[..encoded_len]);
                buf = [0u8; api::ipc::IPC_BUF_SIZE];
            }
            _ => {
                ostd::task::yield_now();
            }
        }
    }
}
