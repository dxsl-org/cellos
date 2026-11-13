#![no_std]
#![no_main]

// Links the Tier-A C library (libc symbols) the C objects call.
extern crate posix_shim;
use abi::declare_manifest;
use ostd::{io::println, syscall::sys_exit};

declare_manifest!(
    block_io = false,
    network = false,
    spawn = true,
    tier = abi::manifest::PROTECTION_CLASS_FFI
);

unsafe extern "C" {
    fn cellos_pthread_witness() -> i32;
}

#[no_mangle]
pub extern "C" fn main() {
    let status = unsafe { cellos_pthread_witness() };
    println(if status == 0 {
        "C-PTHREAD-QEMU: PASS"
    } else {
        "C-PTHREAD-QEMU: FAIL"
    });
    sys_exit(status as usize);
}
