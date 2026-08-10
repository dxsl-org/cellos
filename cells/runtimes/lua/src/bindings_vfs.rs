//! VFS filesystem bindings exposed to Lua via C FFI (`vfs.*`).
//!
//! Uses the typed postcard IPC (`api::ipc::VfsRequest/VfsResponse`) introduced
//! at Milestone 2.1.  The old raw byte-opcode protocol (OP_READ=8, OP_WRITE=4…)
//! was removed from the VFS cell and must not be used here.
//!
//! Reference pattern: `cells/tools/shell/src/cmd_fs.rs` (`vfs_req_ok`, `read_file_vfs`).
// `L` is the universal Lua C API convention for `lua_State*`.
#![allow(non_snake_case)] // reason: L is the Lua C API convention for lua_State pointers

extern crate alloc;

use crate::ffi::{self, LuaState};
#[path = "bindings_vfs_handle_read.rs"]
mod handle_read;

use core::ffi::{c_char, c_int};
use ostd::{clients::VfsClient, service::VfsRef};
/// Safe payload size per IPC call: 512 byte frame minus postcard overhead and path length.
const MAX_CHUNK: usize = 400;

// ─── IPC helpers ──────────────────────────────────────────────────────────────

/// Send a typed VfsRequest to the live VFS service and return `true` when the reply is `Ok`.
pub fn vfs_ok(vfs: &mut VfsRef, req: &api::ipc::VfsRequest<'_>) -> bool {
    let mut reply = [0u8; api::ipc::IPC_BUF_SIZE];
    matches!(
        vfs.call::<api::ipc::VfsRequest, api::ipc::VfsResponse>(req, &mut reply),
        Ok(api::ipc::VfsResponse::Ok)
    )
}

/// Maximum file size read in a single `vfs_get_file_vec` call (64 KB).
const MAX_FILE_READ: usize = 64 * 1024;

/// Read file content from VFS into `out` via bounded handle-addressed reads.
#[allow(dead_code)] // reason: fixed-buffer variant kept beside vfs_get_file_vec for no-alloc callers
pub fn vfs_get_file(path: &str, out: &mut [u8]) -> usize {
    let data = vfs_get_file_vec(path);
    let data_len = data.len().min(out.len());
    out[..data_len].copy_from_slice(&data[..data_len]);
    data_len
}

/// Read file content into a `Vec<u8>` via bounded handle-addressed reads.
pub fn vfs_get_file_vec(path: &str) -> alloc::vec::Vec<u8> {
    let mut vfs = VfsRef::new();
    handle_read::read_file(&mut vfs, path, MAX_FILE_READ).unwrap_or_default()
}

/// Write raw bytes to `path` from Rust (not Lua). Used at startup to install
/// bundled scripts into `/tmp` so `require()` can find them.
pub fn write_bytes(path: &str, data: &[u8]) -> bool {
    vfs_write_chunked(path, data, false)
}

/// Write `data` to `path`, chunking into MAX_CHUNK-byte IPC payloads.
///
/// The first chunk uses `Write` (create/overwrite); subsequent chunks use `Append`.
/// When `append` is true every chunk uses `Append`.
fn vfs_write_chunked(path: &str, data: &[u8], append: bool) -> bool {
    let mut vfs = VfsRef::new();
    if data.is_empty() {
        return if append {
            true
        } else {
            vfs_ok(
                &mut vfs,
                &api::ipc::VfsRequest::Write { path, content: &[] },
            )
        };
    }
    let mut first = !append;
    let mut ok = true;
    for chunk in data.chunks(MAX_CHUNK) {
        let req = if first {
            first = false;
            api::ipc::VfsRequest::Write {
                path,
                content: chunk,
            }
        } else {
            api::ipc::VfsRequest::Append {
                path,
                content: chunk,
            }
        };
        ok &= vfs_ok(&mut vfs, &req);
    }
    ok
}

// ─── Lua argument helpers ─────────────────────────────────────────────────────

/// Read the string arg at stack `idx` as a byte slice borrowed from Lua.
///
/// # Safety
/// `L` must be valid; the slice lives only while the value stays on the Lua stack.
unsafe fn lua_arg_bytes<'a>(L: *mut LuaState, idx: c_int) -> Option<&'a [u8]> {
    let mut len: usize = 0;
    // SAFETY: caller guarantees L is valid; idx is a valid stack position.
    let ptr = unsafe { ffi::lua_tolstring(L, idx, &mut len as *mut _) };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: Lua guarantees `len` valid bytes at `ptr`.
    Some(unsafe { core::slice::from_raw_parts(ptr as *const u8, len) })
}

/// Extract a path `&str` from Lua arg at `idx`. Returns `None` on error.
unsafe fn lua_arg_path<'a>(L: *mut LuaState, idx: c_int) -> Option<&'a str> {
    let raw = unsafe { lua_arg_bytes(L, idx) }?;
    core::str::from_utf8(raw).ok().filter(|s| !s.is_empty())
}

// ─── Core vfs.* Lua bindings ──────────────────────────────────────────────────

/// `vfs.read(path)` → string | nil
///
/// Reads file content from VFS. Returns the content as a Lua string, or nil if
/// the file is missing or empty. Reads are bounded at 64 KiB with no DataPtr fallback.
#[no_mangle]
pub unsafe extern "C" fn vfs_read(L: *mut LuaState) -> c_int {
    let path = match unsafe { lua_arg_path(L, 1) } {
        Some(p) => p,
        None => {
            unsafe { ffi::lua_pushnil(L) };
            return 1;
        }
    };
    let data = vfs_get_file_vec(path);
    if data.is_empty() {
        unsafe { ffi::lua_pushnil(L) };
        return 1;
    }
    // SAFETY: L valid; data contains the initialised file content.
    unsafe { ffi::lua_pushlstring(L, data.as_ptr() as *const c_char, data.len()) };
    1
}

/// `vfs.write(path, content)` → bool
///
/// Creates or overwrites a file. Content larger than 400 bytes is split into
/// multiple Write+Append IPC calls.
#[no_mangle]
pub unsafe extern "C" fn vfs_write(L: *mut LuaState) -> c_int {
    let path = match unsafe { lua_arg_path(L, 1) } {
        Some(p) => p,
        None => {
            unsafe { ffi::lua_pushboolean(L, 0) };
            return 1;
        }
    };
    let content = unsafe { lua_arg_bytes(L, 2) }.unwrap_or(&[]);
    let ok = vfs_write_chunked(path, content, false);
    unsafe { ffi::lua_pushboolean(L, if ok { 1 } else { 0 }) };
    1
}

/// `vfs.append(path, content)` → bool
///
/// Appends content to an existing file (or creates it).
#[no_mangle]
pub unsafe extern "C" fn vfs_append(L: *mut LuaState) -> c_int {
    let path = match unsafe { lua_arg_path(L, 1) } {
        Some(p) => p,
        None => {
            unsafe { ffi::lua_pushboolean(L, 0) };
            return 1;
        }
    };
    let content = unsafe { lua_arg_bytes(L, 2) }.unwrap_or(&[]);
    let ok = vfs_write_chunked(path, content, true);
    unsafe { ffi::lua_pushboolean(L, if ok { 1 } else { 0 }) };
    1
}

/// `vfs.mkdir(path)` → bool
#[no_mangle]
pub unsafe extern "C" fn vfs_mkdir(L: *mut LuaState) -> c_int {
    let path = match unsafe { lua_arg_path(L, 1) } {
        Some(p) => p,
        None => {
            unsafe { ffi::lua_pushboolean(L, 0) };
            return 1;
        }
    };
    let mut vfs = VfsRef::new();
    let ok = vfs_ok(&mut vfs, &api::ipc::VfsRequest::Mkdir(path));
    unsafe { ffi::lua_pushboolean(L, if ok { 1 } else { 0 }) };
    1
}

// ─── Extended vfs.* bindings (Phase 03) ───────────────────────────────────────

/// `vfs.stat(path)` → {size=N, is_dir=bool} | nil
///
/// Returns a table with file metadata, or nil if the path does not exist.
#[no_mangle]
pub unsafe extern "C" fn vfs_stat(L: *mut LuaState) -> c_int {
    let path = match unsafe { lua_arg_path(L, 1) } {
        Some(p) => p,
        None => {
            unsafe { ffi::lua_pushnil(L) };
            return 1;
        }
    };
    let mut vfs = VfsClient::new();
    match vfs.stat(path) {
        Ok((size, is_dir)) => {
            unsafe { ffi::lua_createtable(L, 0, 2) };
            let t = unsafe { ffi::lua_gettop(L) };
            unsafe { ffi::lua_pushinteger(L, size as i64) };
            unsafe { ffi::lua_setfield(L, t, c"size".as_ptr()) };
            unsafe { ffi::lua_pushboolean(L, if is_dir { 1 } else { 0 }) };
            unsafe { ffi::lua_setfield(L, t, c"is_dir".as_ptr()) };
            1
        }
        Err(_) => {
            unsafe { ffi::lua_pushnil(L) };
            1
        }
    }
}

/// `vfs.listdir(path)` → array of "d:name" / "f:name" strings | nil
///
/// Returns a 1-indexed Lua array. Entries are prefixed with `d:` (directory)
/// or `f:` (file). Directories with more than ~30 entries are silently truncated
/// by the 512-byte VFS reply limit.
#[no_mangle]
pub unsafe extern "C" fn vfs_listdir(L: *mut LuaState) -> c_int {
    let path = match unsafe { lua_arg_path(L, 1) } {
        Some(p) => p,
        None => {
            unsafe { ffi::lua_pushnil(L) };
            return 1;
        }
    };
    let mut vfs = VfsClient::new();
    match vfs.list_dir(path) {
        Ok(entries) => {
            let text = core::str::from_utf8(&entries).unwrap_or("");
            unsafe { ffi::lua_createtable(L, 0, 0) };
            let t = unsafe { ffi::lua_gettop(L) };
            let mut i = 1i64;
            for line in text.lines() {
                if line.is_empty() {
                    continue;
                }
                unsafe {
                    ffi::lua_pushlstring(L, line.as_ptr() as *const c_char, line.len());
                    ffi::lua_rawseti(L, t, i);
                }
                i += 1;
            }
            1
        }
        Err(_) => {
            unsafe { ffi::lua_pushnil(L) };
            1
        }
    }
}

/// `vfs.remove(path)` → bool
///
/// Deletes a file from VFS. Returns false if the file does not exist.
#[no_mangle]
pub unsafe extern "C" fn vfs_remove(L: *mut LuaState) -> c_int {
    let path = match unsafe { lua_arg_path(L, 1) } {
        Some(p) => p,
        None => {
            unsafe { ffi::lua_pushboolean(L, 0) };
            return 1;
        }
    };
    let mut vfs = VfsRef::new();
    let ok = vfs_ok(&mut vfs, &api::ipc::VfsRequest::Unlink(path));
    unsafe { ffi::lua_pushboolean(L, if ok { 1 } else { 0 }) };
    1
}
