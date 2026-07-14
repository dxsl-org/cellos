//! x86_64-only C string shim for the littlefs C core.
//!
//! littlefs (`lfs.c`) references strlen/strcpy/strchr/strspn/strcspn. On
//! riscv64/aarch64 these come from the api POSIX shim
//! (`libs/api/src/services/posix/strings.rs`), but that module is deliberately
//! `cfg`-gated OFF on x86_64 to avoid duplicate-symbol collisions with mlibc
//! Tier-B cells. VFS never links mlibc, so providing the five symbols locally
//! is safe here and keeps the api gate intact. mem* come from
//! `compiler_builtins` on every arch.
//!
//! # Law 4 note
//! `unsafe` is required for raw C-pointer walking; each fn documents the
//! contract it inherits from C (`\0`-terminated, valid allocations).
#![allow(unsafe_code)]

use core::ffi::{c_char, c_int};

/// SAFETY contract (all fns): pointers are valid, `\0`-terminated C strings
/// owned by the littlefs C core for the duration of the call (single-threaded
/// VFS dispatch — no concurrent mutation).

#[no_mangle]
pub unsafe extern "C" fn strlen(s: *const c_char) -> usize {
    let mut n = 0usize;
    while unsafe { *s.add(n) } != 0 {
        n += 1;
    }
    n
}

#[no_mangle]
pub unsafe extern "C" fn strcpy(dest: *mut c_char, src: *const c_char) -> *mut c_char {
    let mut i = 0usize;
    loop {
        let c = unsafe { *src.add(i) };
        unsafe { *dest.add(i) = c };
        if c == 0 {
            break;
        }
        i += 1;
    }
    dest
}

#[no_mangle]
pub unsafe extern "C" fn strchr(s: *const c_char, c: c_int) -> *mut c_char {
    let target = c as c_char;
    let mut p = s;
    loop {
        let ch = unsafe { *p };
        if ch == target {
            return p as *mut c_char;
        }
        if ch == 0 {
            return core::ptr::null_mut();
        }
        p = unsafe { p.add(1) };
    }
}

/// Length of the initial segment of `s` consisting only of bytes in `accept`.
#[no_mangle]
pub unsafe extern "C" fn strspn(s: *const c_char, accept: *const c_char) -> usize {
    let mut n = 0usize;
    loop {
        let ch = unsafe { *s.add(n) };
        if ch == 0 {
            return n;
        }
        let mut a = accept;
        let mut found = false;
        loop {
            let ac = unsafe { *a };
            if ac == 0 {
                break;
            }
            if ac == ch {
                found = true;
                break;
            }
            a = unsafe { a.add(1) };
        }
        if !found {
            return n;
        }
        n += 1;
    }
}

/// Length of the initial segment of `s` consisting only of bytes NOT in `reject`.
#[no_mangle]
pub unsafe extern "C" fn strcspn(s: *const c_char, reject: *const c_char) -> usize {
    let mut n = 0usize;
    loop {
        let ch = unsafe { *s.add(n) };
        if ch == 0 {
            return n;
        }
        let mut r = reject;
        loop {
            let rc = unsafe { *r };
            if rc == 0 {
                break;
            }
            if rc == ch {
                return n;
            }
            r = unsafe { r.add(1) };
        }
        n += 1;
    }
}
