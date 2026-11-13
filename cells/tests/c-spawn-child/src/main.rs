#![no_std]
#![no_main]

// Links the Tier-A C library (libc symbols) the C objects call.
extern crate posix_shim;
use abi::declare_manifest;
use ostd::syscall::sys_exit;

// The granted child holds no capability of its own and no launch edge: it can
// read the command line its launcher staged, receive the endpoints that were
// granted, and write to them. Nothing here can start another cell.
declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = abi::manifest::PROTECTION_CLASS_FFI
);

abi::declare_syscalls![Log, Exit, Recv, StateRestore, PipeWrite, Yield];

unsafe extern "C" {
    fn cellos_spawn_child_witness() -> i32;
}

#[no_mangle]
pub extern "C" fn main() {
    let status = unsafe { cellos_spawn_child_witness() };
    sys_exit(status as usize);
}
