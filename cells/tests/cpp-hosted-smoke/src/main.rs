//! `cpp-hosted` witness for x86_64 Tier-2 (ADR-0034, runtime plan R4).
//!
//! The checks live in `cpp/witness.cpp` and print `CPP-HOSTED: <name> OK|FAIL`
//! markers, then `CPP-HOSTED: <pass>/<total> pass`. When every check passed,
//! the Cell ends in a deliberately uncaught exception: libc++abi names it on
//! stderr, `abort()` reaches mlibc's panic path, and the Cell exits with
//! status 1. A run therefore always exits non-zero; the markers, the
//! terminate line and the kernel staying up are the verdict
//! (`scripts/x86/qemu-cpp-hosted-smoke.sh`).

#![no_std]
#![no_main]
extern crate mlibc_shim;
extern crate ostd;
extern crate posix_shim;

use ostd::syscall::{sys_exit, sys_log};

abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = abi::manifest::PROTECTION_CLASS_UNTRUSTED
);
abi::declare_syscalls![
    Write, Log, Exit, GetTime, Yield, Spawn, FutexWait, FutexWake, Open, Close, GetRandom
];

#[cfg(cpp_hosted_witness)]
extern "C" {
    /// Runs every check; returns the number of failed checks. `noexcept` on
    /// the C++ side: no exception reaches this frame (ADR-0034 decision 4).
    fn cpp_hosted_witness() -> i32;
    /// Throws past a `noexcept` boundary: std::terminate, then abort().
    fn cpp_hosted_uncaught();
}

#[cfg(cpp_hosted_witness)]
#[no_mangle]
pub fn main() {
    // SAFETY: integer-only C functions with no arguments (rule H1); mlibc and
    // the C++ static constructors ran in `__cellos_libc_start` before `main`.
    let failed = unsafe { cpp_hosted_witness() };
    if failed != 0 {
        sys_exit(1);
    }
    // SAFETY: as above; it never returns when the runtime is correct.
    unsafe { cpp_hosted_uncaught() };
    let _ = sys_log("CPP-HOSTED: uncaught FAIL (returned)");
    sys_exit(1);
}

/// Built without the C++ runtime (see build.rs): fail loudly instead of passing.
#[cfg(not(cpp_hosted_witness))]
#[no_mangle]
pub fn main() {
    let _ = sys_log("CPP-HOSTED: NOT-BUILT");
    sys_exit(1);
}
