#![no_std]
#![no_main]

use api::declare_manifest;
use ostd::{io::println, syscall::sys_exit};

// No ambient lifecycle authority: the child launch is authorized by the exact
// reviewed `(caller="c-spawn", route=Path|Elf, target="/bin/c-spawn-child")` edge
// in `kernel/src/loader/launch_profile`, never by a SpawnCap this cell does not
// hold. The grant syscalls exist only to fetch that reviewed target's ELF from
// the VFS service, because post-boot the kernel drives no block hardware and the
// cell store belongs to VFS.
declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_FFI
);

api::declare_syscalls![
    Log,
    Exit,
    Send,
    Recv,
    LookupService,
    SpawnFromPath,
    SpawnFromElf,
    Wait,
    StateStash,
    StateRestore,
    GrantAlloc,
    GrantShare,
    GrantFree,
    PipeCreate,
    PipeRead,
    PipeWrite,
    PipeClose,
    PipeShare,
    Yield,
    GetTime
];

unsafe extern "C" {
    fn cellos_spawn_witness(elf_grant: usize, elf_len: usize) -> i32;
}

const CHILD_PATH: &str = "/bin/c-spawn-child";

#[no_mangle]
pub extern "C" fn main() {
    // The launch transaction borrows these bytes; this host owns and frees them.
    let (grant, len) = match ostd::service::lookup(ostd::service::service::VFS)
        .ok_or(())
        .and_then(|vfs| ostd::fs::read_full_via_grant(CHILD_PATH, vfs).map_err(|_| ()))
    {
        Ok(loaded) => loaded,
        Err(()) => {
            println("C-SPAWN-QEMU: FAIL — child ELF unavailable from VFS");
            sys_exit(1);
        }
    };

    let status = unsafe { cellos_spawn_witness(grant, len) };
    ostd::syscall::sys_grant_free(grant);

    println(if status == 0 {
        "C-SPAWN-QEMU: PASS"
    } else {
        "C-SPAWN-QEMU: FAIL"
    });
    sys_exit(status as usize);
}
