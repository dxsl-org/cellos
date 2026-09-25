// SPDX-License-Identifier: MPL-2.0
//! Minimal C++ ABI stubs for freestanding C++ (no exceptions, no RTTI).
//!
//! SAFETY: These stubs ONLY support -fno-exceptions -fno-rtti compiled C++ code.
//! Do NOT attempt to use exceptions, RTTI, or STL containers — they require
//! full libcxxabi/libstdc++ which is SAS-unsafe. Use Tier 3 Linux VM instead.

#![allow(unsafe_code)]

use super::sysio::raw_syscall;
use crate::syscall::ViSyscall;

/// Called when a pure virtual function is invoked (programming error).
/// Terminates the cell immediately — equivalent to a Rust panic.
///
/// # Safety
/// Must only be reached via a compiler-generated vtable thunk for an
/// unoverridden pure virtual slot; never call directly. Never returns.
#[no_mangle]
pub unsafe extern "C" fn __cxa_pure_virtual() -> ! {
    raw_syscall(
        ViSyscall::Log,
        b"FATAL: pure virtual call\n".as_ptr() as usize,
        25,
        0,
        0,
    );
    raw_syscall(ViSyscall::Exit, 134, 0, 0, 0); // 134 = 128 + SIGABRT(6)
    loop {
        core::hint::spin_loop();
    }
}

/// Thread-safe static local init guard — single-threaded stub.
/// C++ emits __cxa_guard_acquire/release around function-local statics.
/// In ViCell's single-threaded cells, a simple flag suffices.
///
/// # Safety
/// `guard` must be a valid, properly aligned pointer to a `u64` owned by the
/// enclosing function-local static's compiler-generated guard variable, and
/// must not be concurrently accessed (single-threaded cells only).
#[no_mangle]
pub unsafe extern "C" fn __cxa_guard_acquire(guard: *mut u64) -> i32 {
    if *guard == 0 {
        1
    } else {
        0
    } // 1 = needs init, 0 = already done
}

/// Marks the function-local static guarded by `guard` as initialized.
///
/// # Safety
/// `guard` must be the same pointer previously passed to a matching
/// `__cxa_guard_acquire` call and must still be valid and properly aligned.
#[no_mangle]
pub unsafe extern "C" fn __cxa_guard_release(guard: *mut u64) {
    *guard = 1; // mark initialized
}

/// Signals that function-local static initialization failed, leaving the
/// guard clear so the next execution retries initialization.
///
/// # Safety
/// `_guard` must be the same pointer previously passed to a matching
/// `__cxa_guard_acquire` call and must still be valid and properly aligned.
#[no_mangle]
pub unsafe extern "C" fn __cxa_guard_abort(_guard: *mut u64) {
    // Init failed — leave guard at 0 so next attempt retries
}

/// Static-destructor registration — accepted and ignored.
///
/// A C++ compiler emits `atexit` (GCC/clang with `-fno-use-cxa-atexit`) or
/// `__cxa_atexit` to run global destructors at process exit. A Cell has no
/// process teardown to run them in: the kernel reclaims the address space on
/// exit, and there is no `exit`-with-destructors path to hang them on. The
/// registration therefore succeeds and never fires, which makes static
/// destructors a documented non-feature of the freestanding C++ profile rather
/// than a silent leak (the frames are reclaimed by the kernel either way).
///
/// # Safety
/// Both functions only accept a callback and return 0; no pointer is
/// dereferenced and no callback is ever invoked.
#[no_mangle]
pub unsafe extern "C" fn atexit(_handler: Option<extern "C" fn()>) -> core::ffi::c_int {
    0
}

/// `__cxa_atexit` — the C++ ABI form of [`atexit`], with the same semantics.
///
/// # Safety
/// As [`atexit`]: arguments are ignored and the handler never runs.
#[no_mangle]
pub unsafe extern "C" fn __cxa_atexit(
    _handler: Option<extern "C" fn(*mut core::ffi::c_void)>,
    _arg: *mut core::ffi::c_void,
    _dso: *mut core::ffi::c_void,
) -> core::ffi::c_int {
    0
}

/// abort() — terminates cell immediately. No cleanup, no atexit handlers.
/// This is the correct behavior for SAS: kernel reclaims all resources.
///
/// # Safety
/// Callable from any context; never returns. Caller-held resources are
/// reclaimed by the kernel on cell exit, not by this function.
#[no_mangle]
pub unsafe extern "C" fn abort() -> ! {
    raw_syscall(ViSyscall::Exit, 134, 0, 0, 0);
    loop {
        core::hint::spin_loop();
    }
}
