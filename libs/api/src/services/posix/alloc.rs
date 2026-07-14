// SPDX-License-Identifier: MPL-2.0
// Memory management: malloc / free / realloc / calloc

#![allow(unsafe_code)]

extern crate alloc;

use core::alloc::Layout;
use core::ffi::c_void;
use core::ptr;

#[repr(C)]
struct AllocHeader {
    size: usize,
    magic: usize,
}

const HEADER_MAGIC: usize = 0xDEAD_C0DE;
const HEADER_SIZE: usize = core::mem::size_of::<AllocHeader>();
const ALIGN: usize = 16;

/// Allocates a block of at least `size` bytes, 16-byte aligned.
///
/// # Safety
/// The returned pointer must eventually be released via `free` or `realloc`
/// from this same allocator (never via the global Rust allocator or a
/// different `malloc` implementation), and must not be freed more than once.
#[no_mangle]
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    malloc_impl(size)
}

/// Releases a block previously allocated by `malloc`, `calloc`, or `realloc`.
///
/// # Safety
/// `ptr` must be null or a pointer previously returned by this allocator's
/// `malloc`/`calloc`/`realloc` family, must not already have been freed, and
/// must not be accessed by the caller after this call returns.
#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut c_void) {
    free_impl(ptr)
}

/// Resizes a previously allocated block, preserving its contents up to
/// `min(old_size, size)` bytes; a `size` of 0 behaves like `free`.
///
/// # Safety
/// `ptr` must be null or a pointer previously returned by this allocator's
/// `malloc`/`calloc`/`realloc` family and not already freed. On success the
/// caller must stop using `ptr` and use only the returned pointer.
#[no_mangle]
pub unsafe extern "C" fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    realloc_impl(ptr, size)
}

/// Allocates zero-initialized storage for `nmemb` elements of `size` bytes each.
///
/// # Safety
/// The returned pointer must eventually be released via `free` or `realloc`
/// from this same allocator, and must not be freed more than once. Overflow
/// of `nmemb * size` is detected internally and yields a null pointer.
#[no_mangle]
pub unsafe extern "C" fn calloc(nmemb: usize, size: usize) -> *mut c_void {
    let total_size = match nmemb.checked_mul(size) {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let p = malloc(total_size);
    if !p.is_null() {
        core::ptr::write_bytes(p as *mut u8, 0, total_size);
    }
    p
}

pub(super) unsafe fn malloc_impl(size: usize) -> *mut c_void {
    let total = match size.checked_add(HEADER_SIZE) {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let layout = match Layout::from_size_align(total, ALIGN) {
        Ok(l) => l,
        Err(_) => return ptr::null_mut(),
    };
    let raw = alloc::alloc::alloc(layout);
    if raw.is_null() {
        return ptr::null_mut();
    }
    let header = raw as *mut AllocHeader;
    (*header).size = size;
    (*header).magic = HEADER_MAGIC;
    raw.add(HEADER_SIZE) as *mut c_void
}

pub(super) unsafe fn free_impl(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    let raw = (ptr as *mut u8).sub(HEADER_SIZE);
    let header = raw as *mut AllocHeader;
    if (*header).magic != HEADER_MAGIC {
        return;
    }
    let total = (*header).size + HEADER_SIZE;
    let layout = Layout::from_size_align_unchecked(total, ALIGN);
    alloc::alloc::dealloc(raw, layout);
}

unsafe fn realloc_impl(ptr: *mut c_void, new_size: usize) -> *mut c_void {
    if ptr.is_null() {
        return malloc_impl(new_size);
    }
    if new_size == 0 {
        free_impl(ptr);
        return ptr::null_mut();
    }
    let raw = (ptr as *mut u8).sub(HEADER_SIZE);
    let header = raw as *mut AllocHeader;
    if (*header).magic != HEADER_MAGIC {
        return ptr::null_mut();
    }
    let old_size = (*header).size;
    let old_layout = Layout::from_size_align_unchecked(old_size + HEADER_SIZE, ALIGN);
    let total_new = match new_size.checked_add(HEADER_SIZE) {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let new_raw = alloc::alloc::realloc(raw, old_layout, total_new);
    if new_raw.is_null() {
        return ptr::null_mut();
    }
    let new_header = new_raw as *mut AllocHeader;
    (*new_header).size = new_size;
    (*new_header).magic = HEADER_MAGIC;
    new_raw.add(HEADER_SIZE) as *mut c_void
}

// ---------------------------------------------------------------------------
// C++ operator new/delete stubs
// ---------------------------------------------------------------------------

/// C++ `operator new(size_t)` — same allocation semantics as `malloc`.
///
/// # Safety
/// The returned pointer must eventually be released via `_ZdlPv`/`_ZdlPvm`
/// (`operator delete`), never via `free` or a mismatched deallocator, and
/// must not be freed more than once.
#[no_mangle]
pub unsafe extern "C" fn _Znwm(size: usize) -> *mut c_void {
    malloc_impl(size)
}

/// C++ `operator delete(void*)` — same semantics as `free`.
///
/// # Safety
/// `ptr` must be null or a pointer previously returned by `_Znwm`, must not
/// already have been freed, and must not be accessed after this call.
#[no_mangle]
pub unsafe extern "C" fn _ZdlPv(ptr: *mut c_void) {
    free_impl(ptr)
}

/// C++ sized `operator delete(void*, size_t)`; the size is compiler-provided
/// bookkeeping and is unused here since the allocator header already tracks it.
///
/// # Safety
/// `ptr` must be null or a pointer previously returned by `_Znwm`, must not
/// already have been freed, and `_size` must match the size originally
/// requested from `_Znwm`.
#[no_mangle]
pub unsafe extern "C" fn _ZdlPvm(ptr: *mut c_void, _size: usize) {
    free_impl(ptr)
}

/// C++ `operator new[](size_t)` — same allocation semantics as `malloc`.
///
/// # Safety
/// The returned pointer must eventually be released via `_ZdaPv`/`_ZdaPvm`
/// (`operator delete[]`), never via `free` or a mismatched deallocator, and
/// must not be freed more than once.
#[no_mangle]
pub unsafe extern "C" fn _Znam(size: usize) -> *mut c_void {
    malloc_impl(size)
}

/// C++ `operator delete[](void*)` — same semantics as `free`.
///
/// # Safety
/// `ptr` must be null or a pointer previously returned by `_Znam`, must not
/// already have been freed, and must not be accessed after this call.
#[no_mangle]
pub unsafe extern "C" fn _ZdaPv(ptr: *mut c_void) {
    free_impl(ptr)
}

/// C++ sized `operator delete[](void*, size_t)`; the size is compiler-provided
/// bookkeeping and is unused here since the allocator header already tracks it.
///
/// # Safety
/// `ptr` must be null or a pointer previously returned by `_Znam`, must not
/// already have been freed, and `_size` must match the size originally
/// requested from `_Znam`.
#[no_mangle]
pub unsafe extern "C" fn _ZdaPvm(ptr: *mut c_void, _size: usize) {
    free_impl(ptr)
}
