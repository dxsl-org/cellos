//! Lua REPL session — wraps `ostd::repl::Repl` with Lua-specific logic.
//!
//! Handles multi-line continuation (detects incomplete chunks) and maintains
//! the accumulation buffer so the user can type multi-line functions.

extern crate alloc;

use crate::ffi::{LuaState, LUA_OK};
use alloc::string::String;
use core::ffi::CStr;
use ostd::repl::{ReadResult, Repl};

/// Prompt strings.
const PROMPT1: &str = "> "; // first line of an expression
const PROMPT2: &str = ">> "; // continuation line

/// Run an interactive Lua REPL until the user presses Ctrl+D.
///
/// # Safety
/// `L` must be a valid, non-null Lua state allocated by `luaL_newstate`.
#[allow(non_snake_case)] // reason: L is the Lua API convention
pub unsafe fn run_repl(L: *mut LuaState) {
    let mut repl = Repl::new();
    let mut buf = String::new();

    loop {
        let prompt = if buf.is_empty() { PROMPT1 } else { PROMPT2 };
        match repl.read_line(prompt) {
            ReadResult::Eof => break,
            ReadResult::Interrupted => {
                buf.clear();
                continue;
            }
            ReadResult::Line(line) => {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(&line);

                // Try to compile as a complete chunk.
                if !try_eval(L, &buf) {
                    // If incomplete (multi-line), keep accumulating.
                    // If an actual error, clear the buffer.
                    if !is_incomplete(L) {
                        buf.clear();
                    }
                } else {
                    buf.clear();
                }

                // Pop any leftover values.
                let top = unsafe { crate::ffi::lua_gettop(L) };
                if top > 0 {
                    unsafe { crate::ffi::lua_settop(L, 0) };
                }
            }
        }
    }
}

/// Compile and run a single chunk of Lua source (used for `lua -e <code>`).
/// Returns `true` on success; prints the Lua error and returns `false` on failure.
///
/// # Safety
/// `L` must be a valid, non-null Lua state.
#[allow(non_snake_case)]
pub unsafe fn eval(L: *mut LuaState, code: &str) -> bool {
    // SAFETY: forwarded to try_eval which upholds the same contract on L.
    unsafe { try_eval(L, code) }
}

/// Returns `true` if `code` compiled and ran successfully.
///
/// # Safety
/// `L` must be a valid, non-null Lua state.
#[allow(non_snake_case)]
unsafe fn try_eval(L: *mut LuaState, code: &str) -> bool {
    let mut nul_buf = alloc::vec![0u8; code.len() + 1];
    nul_buf[..code.len()].copy_from_slice(code.as_bytes());
    // Same chunk text and name `luaL_loadstring` used (source up to the first
    // NUL), but compiled text-only: console and `-e` input never reach the
    // bytecode undumper.
    let Ok(source) = CStr::from_bytes_until_nul(&nul_buf) else {
        return false; // unreachable: nul_buf always ends with NUL
    };

    // SAFETY: L is non-null; `source` is NUL-terminated and outlives the call.
    let rc = unsafe { crate::ffi::load_text(L, source.to_bytes(), source) };
    if rc != LUA_OK {
        print_error(L);
        return false;
    }

    // lua_pcall(L,0,MULTRET,0) == lua_pcallk(L,0,MULTRET,0, 0, NULL)
    let rc = unsafe {
        crate::ffi::lua_pcallk(L, 0, crate::ffi::LUA_MULTRET, 0, 0, core::ptr::null_mut())
    };
    if rc != LUA_OK {
        print_error(L);
        return false;
    }
    true
}

/// Check if the last compile error looks like an incomplete chunk.
///
/// # Safety
/// `L` must be a valid, non-null Lua state; the top of the stack must hold
/// the error message from a failed `load_text`.
#[allow(non_snake_case)]
unsafe fn is_incomplete(L: *mut LuaState) -> bool {
    let mut len = 0usize;
    // SAFETY: -1 is the top of stack; L is non-null.
    let ptr = unsafe { crate::ffi::lua_tolstring(L, -1, &mut len as *mut _) };
    if ptr.is_null() {
        return false;
    }
    // SAFETY: ptr points to `len` valid bytes (Lua-managed string).
    let bytes = unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len) };
    // Lua reports incomplete chunks with "<eof>" in the error message.
    bytes.windows(5).any(|w| w == b"<eof>")
}

/// Print the error at stack top to stderr (serial console).
///
/// # Safety
/// `L` must be a valid, non-null Lua state with an error string on top.
#[allow(non_snake_case)]
pub(crate) unsafe fn print_error(L: *mut LuaState) {
    let mut len = 0usize;
    // SAFETY: -1 is the error at top of stack.
    let ptr = unsafe { crate::ffi::lua_tolstring(L, -1, &mut len as *mut _) };
    if !ptr.is_null() {
        // SAFETY: ptr is a valid Lua-managed byte slice of length `len`.
        let bytes = unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len) };
        if let Ok(s) = core::str::from_utf8(bytes) {
            ostd::io::println(s);
        }
    }
    // Pop the error.
    unsafe { crate::ffi::lua_settop(L, -2) };
}
