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
    let mut bytes = unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len) };
    while !bytes.is_empty() {
        match core::str::from_utf8(bytes) {
            Ok(text) => {
                ostd::io::print(text);
                break;
            }
            Err(err) => {
                let good = err.valid_up_to();
                if good != 0 {
                    // SAFETY: `valid_up_to` covers only valid UTF-8.
                    ostd::io::print(unsafe { core::str::from_utf8_unchecked(&bytes[..good]) });
                }
                ostd::io::print("\u{fffd}");
                bytes = &bytes[(good + err.error_len().unwrap_or(bytes.len() - good))..];
            }
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
