//! libdrm-lite witness for x86_64 Tier-2 (runtime plan R6).
//!
//! The checks live in `c/witness.c` and print `DRM-LITE: <name> OK|FAIL`
//! markers, then `DRM-LITE: <pass>/<total> pass`; the Cell exits 0 only when
//! every check passed. They drive libdrm-lite and the mlibc fd provider route
//! against the test-only fake provider; the iGPU G3 transport replaces it.
//!
//! Running it needs a launch row for `/bin/drm-lite-x86` (owner H-1); the
//! runner is `cells/tests/drm-lite-x86/run-qemu-x86.sh`.

#![no_std]
#![no_main]
extern crate api;
extern crate drm_cellos;
extern crate mlibc_shim;
extern crate ostd;

use ostd::syscall::sys_exit;

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_UNTRUSTED
);
api::declare_syscalls![
    Write, Log, Exit, GetTime, Yield, FutexWait, FutexWake, Open, Read, Close, Fstat, GetRandom
];

#[cfg(drm_lite_witness)]
extern "C" {
    /// Runs every check; 0 when all pass. Integer-only signature (rule H1).
    fn drm_lite_witness() -> i32;
}

#[cfg(drm_lite_witness)]
#[no_mangle]
pub fn main() {
    // SAFETY: plain C function with no arguments; mlibc is initialised by the
    // libc start-up hook before `main`.
    let failed = unsafe { drm_lite_witness() };
    sys_exit(if failed == 0 { 0 } else { 1 });
}

/// Built without mlibc (see build.rs): fail loudly instead of passing.
#[cfg(not(drm_lite_witness))]
#[no_mangle]
pub fn main() {
    let _ = ostd::syscall::sys_log("DRM-LITE: NOT-BUILT");
    sys_exit(1);
}
