//! mlibc `posix` witness for x86_64 Tier-2 (runtime plan R3a).
//!
//! The checks live in `c/witness.c` and print `MLIBC-POSIX: <name> OK|FAIL`
//! markers, then `MLIBC-POSIX: <pass>/<total> pass`; the Cell exits 0 only
//! when every check passed.
//!
//! Running it needs the Cell initial stack with auxv and ostd's
//! `__cellos_libc_start` hook (CP1 `cp1-cell-initial-stack-auxv.md`) plus a
//! launch row (H-1). Until then this crate is a link proof.

#![no_std]
#![no_main]
extern crate mlibc_shim;
extern crate ostd;
extern crate posix_shim;

use ostd::syscall::sys_exit;

abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = abi::manifest::PROTECTION_CLASS_UNTRUSTED
);
abi::declare_syscalls![
    Write, Log, Exit, GetTime, Yield, Spawn, FutexWait, FutexWake, Open, Close, GetRandom
];

#[cfg(mlibc_witness)]
extern "C" {
    /// Runs every check; 0 when all pass. Integer-only signature (rule H1).
    fn mlibc_posix_witness() -> i32;
}

#[cfg(mlibc_witness)]
#[no_mangle]
pub fn main() {
    // SAFETY: plain C function with no arguments; mlibc is initialised by the
    // libc start-up hook before `main` (see the module docs).
    let failed = unsafe { mlibc_posix_witness() };
    sys_exit(if failed == 0 { 0 } else { 1 });
}

/// Built without mlibc (see build.rs): fail loudly instead of passing.
#[cfg(not(mlibc_witness))]
#[no_mangle]
pub fn main() {
    let _ = ostd::syscall::sys_log("MLIBC-POSIX: NOT-BUILT");
    sys_exit(1);
}
