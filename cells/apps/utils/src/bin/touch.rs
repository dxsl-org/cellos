#![no_std]
#![no_main]
extern crate ostd;

/// touch stub — creates an empty file. Requires VFS write path (Phase 13 FAT32)
/// and arg-passing (Phase 17a).
#[no_mangle]
pub fn main() {
    ostd::io::println("touch: VFS write path not yet available (Phase 13 FAT32)");
    ostd::syscall::sys_exit(1);
}
