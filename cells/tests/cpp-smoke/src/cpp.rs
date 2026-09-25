//! C++ FFI boundary for the `cpp-freestanding` profile smoke cell.
//!
//! Every `unsafe` in this crate lives here, behind safe wrappers that take and
//! return plain values, so the host logic in `main.rs` reads as ordinary Rust.
//! This file and the crate are both named in `scripts/unsafe-allowlist.toml`
//! (class `c-ffi`): a C++ translation unit is outside the Rust type system by
//! construction, exactly like the C FFI cells that came before it.

extern "C" {
    fn cpp_static_ctor_marker() -> u32;
    fn cpp_static_dtor_marker() -> u32;
    fn cpp_virtual_dispatch_total() -> i32;
    fn cpp_virtual_delete() -> i32;
    fn cpp_template_total() -> i32;
    fn cpp_heap_churn(iterations: i32) -> i32;
    fn cpp_read_file(path: *const u8, buf: *mut u8, len: usize) -> i32;
}

pub fn static_ctor_marker() -> u32 {
    unsafe { cpp_static_ctor_marker() }
}

pub fn static_dtor_marker() -> u32 {
    unsafe { cpp_static_dtor_marker() }
}

pub fn virtual_dispatch_total() -> i32 {
    unsafe { cpp_virtual_dispatch_total() }
}

pub fn virtual_delete_area() -> i32 {
    unsafe { cpp_virtual_delete() }
}

pub fn template_total() -> i32 {
    unsafe { cpp_template_total() }
}

pub fn heap_churn(iterations: i32) -> i32 {
    unsafe { cpp_heap_churn(iterations) }
}

/// Read `path` (NUL-terminated) into `buf` through the POSIX shim's C ABI.
/// Returns the byte count, or a negative error code from `open`/`read`.
pub fn read_file(path: &[u8], buf: &mut [u8]) -> i32 {
    unsafe { cpp_read_file(path.as_ptr(), buf.as_mut_ptr(), buf.len()) }
}
