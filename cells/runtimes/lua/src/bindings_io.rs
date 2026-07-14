//! `io.*` and `os.*` Lua bindings backed by ViCell VFS and syscalls.
//!
//! `io.open` and `io.write` are implemented as a Lua-level wrapper injected at
//! startup (see `main.rs`:`inject_io_setup`).  This module only provides the
//! single primitive C function `ViCell_io_write` that the Lua wrapper calls.
//!
//! `os.execute` spawns a binary via `SpawnFromPath` — no args support yet.
// `L` is the universal Lua C API convention for `lua_State*`.
#![allow(non_snake_case)] // reason: L is the Lua C API convention for lua_State pointers

use crate::ffi::LuaState;
use core::ffi::c_int;

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
        let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len) };
        if let Ok(s) = core::str::from_utf8(bytes) {
            ostd::io::print(s);
        }
    }
    0
}

// ─── os.execute ──────────────────────────────────────────────────────────────

/// `os.execute(cmd)` — spawn `cmd` as an ELF binary via `SpawnFromPath`.
///
/// Bare names are prefixed with `/bin/`.  Returns 0 on success, 1 on failure.
/// Arguments are not yet forwarded to the spawned binary.
#[no_mangle]
pub unsafe extern "C" fn ViCell_os_execute(L: *mut LuaState) -> c_int {
    let mut path_buf = [0u8; 512];
    let mut len: usize = 0;
    // SAFETY: L is non-null; stack index 1 holds the optional command string.
    let ptr = unsafe { crate::ffi::lua_tolstring(L, 1, &mut len as *mut _) };
    if ptr.is_null() || len == 0 {
        // os.execute() with no arg returns true (a shell is available).
        // SAFETY: L is non-null.
        unsafe { crate::ffi::lua_pushboolean(L, 1) };
        return 1;
    }
    len = len.min(511);
    // SAFETY: Lua guarantees `len` valid bytes at `ptr`.
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len) };
    path_buf[..len].copy_from_slice(bytes);
    let cmd = core::str::from_utf8(&path_buf[..len]).unwrap_or("");

    let mut resolved = alloc::string::String::new();
    let path = if cmd.starts_with('/') {
        cmd
    } else {
        resolved.push_str("/bin/");
        resolved.push_str(cmd.split_whitespace().next().unwrap_or(cmd));
        resolved.as_str()
    };

    let exit_code = match ostd::syscall::sys_spawn_from_path(path) {
        ostd::syscall::SyscallResult::Ok(_) => 0i64,
        ostd::syscall::SyscallResult::Err(_) => {
            ostd::io::print("[lua] os.execute: failed to spawn '");
            ostd::io::print(path);
            ostd::io::println("'");
            1i64
        }
    };
    // SAFETY: L is non-null.
    unsafe { crate::ffi::lua_pushinteger(L, exit_code) };
    1
}

extern crate alloc;
