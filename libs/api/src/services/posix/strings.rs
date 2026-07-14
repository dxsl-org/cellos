// SPDX-License-Identifier: MPL-2.0
// String and memory primitives

#![allow(unsafe_code)]

use core::ffi::{c_char, c_int, c_void};

/// Copy `n` bytes from `src` to `dest`.
///
/// # Safety
/// `dest` and `src` must each be valid for `n` bytes of reads/writes respectively, and the
/// two regions must not overlap (use `memmove` if they might).
#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let d = dest as *mut u8;
    let s = src as *const u8;
    for i in 0..n {
        *d.add(i) = *s.add(i);
    }
    dest
}

/// Copy `n` bytes from `src` to `dest`, correctly handling overlapping regions.
///
/// # Safety
/// `dest` and `src` must each be valid for `n` bytes of reads/writes respectively. Overlap
/// between the two regions is permitted.
#[no_mangle]
pub unsafe extern "C" fn memmove(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let d = dest as *mut u8;
    let s = src as *const u8;
    if (s as usize) < (d as usize) {
        let mut i = n;
        while i > 0 {
            i -= 1;
            *d.add(i) = *s.add(i);
        }
    } else {
        for i in 0..n {
            *d.add(i) = *s.add(i);
        }
    }
    dest
}

/// Fill the first `n` bytes of `s` with the low byte of `c`.
///
/// # Safety
/// `s` must be valid for `n` bytes of writes.
#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut c_void, c: c_int, n: usize) -> *mut c_void {
    let d = s as *mut u8;
    let v = c as u8;
    for i in 0..n {
        *d.add(i) = v;
    }
    s
}

/// Compare the first `n` bytes of `s1` and `s2`.
///
/// # Safety
/// `s1` and `s2` must each be valid for `n` bytes of reads.
#[no_mangle]
pub unsafe extern "C" fn memcmp(s1: *const c_void, s2: *const c_void, n: usize) -> c_int {
    let s1 = core::slice::from_raw_parts(s1 as *const u8, n);
    let s2 = core::slice::from_raw_parts(s2 as *const u8, n);
    for i in 0..n {
        let diff = s1[i] as c_int - s2[i] as c_int;
        if diff != 0 {
            return diff;
        }
    }
    0
}

/// Return the length of the NUL-terminated string `s`, excluding the terminator.
///
/// # Safety
/// `s` must point to a valid, NUL-terminated string readable up to and including the
/// terminator.
#[no_mangle]
pub unsafe extern "C" fn strlen(s: *const c_char) -> usize {
    let mut len = 0;
    while *s.add(len) != 0 {
        len += 1;
    }
    len
}

/// Copy the NUL-terminated string `src` into `dest`, including the terminator.
///
/// # Safety
/// `src` must be a valid NUL-terminated string. `dest` must be valid for writes of at least
/// `strlen(src) + 1` bytes. The two buffers must not overlap.
#[no_mangle]
pub unsafe extern "C" fn strcpy(dest: *mut c_char, src: *const c_char) -> *mut c_char {
    let mut i = 0;
    loop {
        let c = *src.add(i);
        *dest.add(i) = c;
        if c == 0 {
            break;
        }
        i += 1;
    }
    dest
}

/// Copy at most `n` bytes from the NUL-terminated string `src` into `dest`, zero-padding any
/// remaining bytes when `src` is shorter than `n`.
///
/// # Safety
/// `src` must be a valid NUL-terminated string readable up to its terminator or `n` bytes,
/// whichever comes first. `dest` must be valid for writes of `n` bytes. The two buffers must
/// not overlap.
#[no_mangle]
pub unsafe extern "C" fn strncpy(dest: *mut c_char, src: *const c_char, n: usize) -> *mut c_char {
    let mut i = 0;
    while i < n {
        let c = *src.add(i);
        *dest.add(i) = c;
        if c == 0 {
            while i < n {
                *dest.add(i) = 0;
                i += 1;
            }
            break;
        }
        i += 1;
    }
    dest
}

/// Lexicographically compare NUL-terminated strings `s1` and `s2`.
///
/// # Safety
/// `s1` and `s2` must each be valid NUL-terminated strings readable up to their terminator.
#[no_mangle]
pub unsafe extern "C" fn strcmp(s1: *const c_char, s2: *const c_char) -> c_int {
    let mut i = 0;
    loop {
        let c1 = *s1.add(i);
        let c2 = *s2.add(i);
        if c1 != c2 {
            return c1 as c_int - c2 as c_int;
        }
        if c1 == 0 {
            return 0;
        }
        i += 1;
    }
}

/// Compare at most `n` bytes of the NUL-terminated strings `s1` and `s2`.
///
/// # Safety
/// `s1` and `s2` must each be readable up to their terminator or `n` bytes, whichever comes
/// first.
#[no_mangle]
pub unsafe extern "C" fn strncmp(s1: *const c_char, s2: *const c_char, n: usize) -> c_int {
    for i in 0..n {
        let c1 = *s1.add(i);
        let c2 = *s2.add(i);
        if c1 != c2 {
            return c1 as c_int - c2 as c_int;
        }
        if c1 == 0 {
            return 0;
        }
    }
    0
}

/// Append the NUL-terminated string `src` to the end of the NUL-terminated string `dest`.
///
/// # Safety
/// `dest` must be a valid NUL-terminated string with enough trailing capacity to hold
/// `strlen(dest) + strlen(src) + 1` bytes. `src` must be a valid NUL-terminated string. The
/// two buffers must not overlap.
#[no_mangle]
pub unsafe extern "C" fn strcat(dest: *mut c_char, src: *const c_char) -> *mut c_char {
    let len = strlen(dest);
    strcpy(dest.add(len), src);
    dest
}

/// Find the first occurrence of byte `c` in the NUL-terminated string `s`, returning null if
/// not found (the search includes the terminator when `c == 0`).
///
/// # Safety
/// `s` must be a valid NUL-terminated string readable up to its terminator.
#[no_mangle]
pub unsafe extern "C" fn strchr(s: *const c_char, c: c_int) -> *mut c_char {
    let v = c as u8;
    let mut i = 0;
    loop {
        let b = *s.add(i);
        if b == v {
            return s.add(i) as *mut c_char;
        }
        if b == 0 {
            return core::ptr::null_mut();
        }
        i += 1;
    }
}

/// Find the last occurrence of byte `c` in the NUL-terminated string `s`, returning null if
/// not found (the search includes the terminator when `c == 0`).
///
/// # Safety
/// `s` must be a valid NUL-terminated string readable up to its terminator.
#[no_mangle]
pub unsafe extern "C" fn strrchr(s: *const c_char, c: c_int) -> *mut c_char {
    let v = c as u8;
    let len = strlen(s);
    let mut i = len;
    loop {
        if *s.add(i) == v {
            return s.add(i) as *mut c_char;
        }
        if i == 0 {
            return core::ptr::null_mut();
        }
        i -= 1;
    }
}

/// Find the first occurrence of byte `c` within the first `n` bytes of `s`, returning null if
/// not found.
///
/// # Safety
/// `s` must be valid for `n` bytes of reads.
#[no_mangle]
pub unsafe extern "C" fn memchr(s: *const c_void, c: c_int, n: usize) -> *mut c_void {
    let v = c as u8;
    let p = s as *const u8;
    for i in 0..n {
        if *p.add(i) == v {
            return p.add(i) as *mut c_void;
        }
    }
    core::ptr::null_mut()
}
