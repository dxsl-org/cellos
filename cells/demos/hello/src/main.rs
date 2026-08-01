#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate ostd;

api::declare_syscalls![Log];

ostd::cell_main!(cell_main);

fn cell_main() {
    ostd::io::println("Hello form separate ELF!");
    ostd::syscall::sys_exit(0);
}
