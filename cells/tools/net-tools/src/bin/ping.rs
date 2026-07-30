#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

api::declare_syscalls![Log];

ostd::cell_main!(cell_main);

/// ping <host> — ICMP echo (stub; requires network + ICMP socket from Phase 15 data path).
fn cell_main() {
    ostd::io::println("ping: ICMP socket data path not yet wired (Phase 15 data-path milestone)");
    ostd::syscall::sys_exit(1);
}
