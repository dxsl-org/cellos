#![no_std]
#![no_main]
#![forbid(unsafe_code)]

use ostd::syscall::sys_exit;

// This test-hooks image-only cell holds no authority and exits on its first run.
abi::declare_manifest!(block_io = false, network = false, spawn = false);
abi::declare_syscalls![Exit];

ostd::cell_main!(cell_main);

fn cell_main() {
    sys_exit(0);
}
