#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

ostd::cell_main!(cell_main);

/// free — print memory usage summary (approximate until MemInfo syscall lands).
fn cell_main() {
    ostd::io::println("              total        used        free");
    ostd::io::println("Mem:        131072       ~4096     ~127000 (KB approx)");
    ostd::io::println("Note: MemInfo syscall not yet wired — showing estimated values.");
    ostd::syscall::sys_exit(0);
}
