//! `io.*` Lua bindings backed by the ViCell console.
//!
//! `io.open` and `io.write` are implemented as a Lua-level wrapper injected at
//! startup (see `main.rs`:`inject_io_setup`).  This module only provides the
//! single primitive C function `ViCell_io_write` that the Lua wrapper calls.
//! `os.execute` is stripped by the sandbox prelude and has no binding.
// `L` is the universal Lua C API convention for `lua_State*`.
#![allow(non_snake_case)] // reason: L is the Lua C API convention for lua_State pointers

use crate::ffi::LuaState;
use core::ffi::c_int;

/// Console output for Lua's C library. `Log` is the only output authority of
/// this cell; `_write` would require the unrelated filesystem Write syscall.
///
/// # Safety
/// `ptr` points to `len` readable bytes for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn lua_console_write(ptr: *const core::ffi::c_char, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    // SAFETY: the caller guarantees `ptr` addresses `len` readable bytes for
    // this call, and nothing here outlives it.
    let bytes = unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len) };
    for chunk in bytes.utf8_chunks() {
        if !chunk.valid().is_empty() {
            ostd::io::print(chunk.valid());
        }
        if !chunk.invalid().is_empty() {
            ostd::io::print("\u{fffd}");
        }
    }
}

// ─── io.write primitive ───────────────────────────────────────────────────────

/// `ViCell_io_write(str)` — write a string to the serial console.
///
/// Called by the Lua-level `io.write` wrapper injected at startup.  Accepts
/// exactly one string argument; the wrapper handles variadic args and `tostring`.
#[no_mangle]
pub unsafe extern "C" fn ViCell_io_write(L: *mut LuaState) -> c_int {
    let mut len: usize = 0;
    // SAFETY: L is non-null; stack index 1 holds the string argument.
    let ptr = unsafe { crate::ffi::lua_tolstring(L, 1, &mut len as *mut _) };
    if !ptr.is_null() && len > 0 {
        // SAFETY: Lua guarantees `len` valid bytes at `ptr`.
        let bytes = unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len) };
        if let Ok(s) = core::str::from_utf8(bytes) {
            ostd::io::print(s);
        }
    }
    0
}
