#![no_std]
#![no_main]
extern crate ostd;

/// mv stub — requires arg-passing (Phase 17a) and VFS write (Phase 13 FAT32).
#[no_mangle]
pub fn main() {
    ostd::io::println("mv: arg-passing not yet wired (Phase 17a)");
    ostd::syscall::sys_exit(1);
}
