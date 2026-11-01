//! Fault-inject builds only: genuine user faults (not exits) so the kernel
//! retires the whole Cell incarnation exactly as for a real crash.

#![allow(unsafe_code)]

use alloc::format;
use api::ipc::{VfsRequest, VfsResponse};
use ostd::io::println;
use ostd::syscall::{self, SyscallResult};
use recovery_demo::{crash, HOLD_LEASE_PATH};

use crate::common;

/// Load from an unmapped user address: a page fault the kernel attributes to
/// this Cell and answers with whole-generation retirement.
pub fn crash() -> ! {
    // SAFETY: intentionally invalid; the access traps before any value exists.
    let value = unsafe { core::ptr::read_volatile(0x10 as *const u64) };
    panic!("unreachable fault result {value}");
}

/// `arg` is the authority-lent grant for `LOCK_AND_DIE`, unused otherwise.
pub fn scenario(mode: u8, arg: u64) -> ! {
    match mode {
        crash::SPIN => {
            let _ = ostd::task::spawn(|| loop {
                core::hint::spin_loop();
            });
            common::sleep(5);
            println("[recovery-worker] crash: root faults while a helper spins");
            crash()
        }
        crash::HOLD_LEASE => hold_lease(),
        crash::SHM_EXPORT => {
            if let SyscallResult::Ok(handle) = syscall::sys_shm_alloc(4096) {
                let mapped = matches!(syscall::sys_shm_map(handle, 0), SyscallResult::Ok(_));
                println(&format!("[recovery-worker] crash: shm export handle={handle:#x} mapped={mapped}"));
            }
            crash()
        }
        crash::LOCK_AND_DIE => {
            let held = syscall::sys_grant_copy_from_slice(arg as usize, &1u64.to_le_bytes());
            println(&format!("[recovery-worker] crash: lock word set in authority grant ({held:?})"));
            crash()
        }
        _ => crash(),
    }
}

/// A helper faults while the root thread is parked in a VFS read whose grant
/// lease the `recovery-fixture` VFS withholds, so owner death finds a live
/// borrower that has not acknowledged.
fn hold_lease() -> ! {
    let Some(vfs) = common::lookup(api::syscall::service::VFS) else { crash() };
    let Some(grant) = syscall::sys_grant_alloc(4096) else { crash() };
    syscall::sys_grant_share(grant, vfs, 2);
    let _ = ostd::task::spawn(|| {
        common::sleep(30);
        println("[recovery-worker] crash: helper faults while VFS holds the lease");
        crash()
    });
    let mut send = [0u8; 256];
    let mut recv = [0u8; 64];
    let request = VfsRequest::ReadFileGrant { path: HOLD_LEASE_PATH, grant, max: 4096 };
    let reply = ostd::ipc::service_call_typed::<_, VfsResponse>(vfs, &request, &mut send, &mut recv);
    println(&format!("[recovery-worker] hold-lease read returned early: {}", reply.is_ok()));
    crash()
}
