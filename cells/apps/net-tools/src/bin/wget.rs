#![no_std]
#![no_main]
extern crate ostd;

/// wget <url> — HTTP download (stub; Phase 15 data-path + VFS write).
#[no_mangle]
pub fn main() {
    ostd::io::println("wget: TCP + VFS write not yet wired (Phase 15 + Phase 13 FAT32)");
    ostd::syscall::sys_exit(1);
}
