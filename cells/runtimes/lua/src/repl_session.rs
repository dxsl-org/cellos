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

                // Compile and run; an incomplete chunk keeps accumulating,
                // anything else (success or a reported error) starts afresh.
                // SAFETY: L is the valid state run_repl's caller guarantees.
                if unsafe { try_eval(L, &buf, true) } != Outcome::Incomplete {
                    buf.clear();
                }

                // Pop any leftover values (a chunk's results).
                // SAFETY: L is valid (run_repl's contract).
                let top = unsafe { crate::ffi::lua_gettop(L) };
                if top > 0 {
                    // SAFETY: L is valid; settop(0) only shrinks the stack.
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
    // Without continuation lines an incomplete chunk is an ordinary error.
    unsafe { try_eval(L, code, false) == Outcome::Ok }
}

/// How [`try_eval`] ended. Every outcome leaves the stack as it found it
/// apart from the chunk's own results on `Ok`.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// The chunk compiled and ran.
    Ok,
    /// The chunk ends mid-statement; more input may complete it. Nothing was printed.
    Incomplete,
    /// Compilation or execution failed; the error was printed.
    Failed,
}

/// Compile and run `code`. With `continuation`, a chunk that only failed to
/// compile because it ends early is reported as [`Outcome::Incomplete`]
/// instead of printed, so the REPL can ask for more lines.
///
/// # Safety
/// `L` must be a valid, non-null Lua state.
#[allow(non_snake_case)]
unsafe fn try_eval(L: *mut LuaState, code: &str, continuation: bool) -> Outcome {
    let mut nul_buf = alloc::vec![0u8; code.len() + 1];
    nul_buf[..code.len()].copy_from_slice(code.as_bytes());
    // Same chunk text and name `luaL_loadstring` used (source up to the first
    // NUL), but compiled text-only: console and `-e` input never reach the
    // bytecode undumper.
    let Ok(source) = CStr::from_bytes_until_nul(&nul_buf) else {
        return Outcome::Failed; // unreachable: nul_buf always ends with NUL
    };

    // SAFETY: L is non-null; `source` is NUL-terminated and outlives the call.
    let rc = unsafe { crate::ffi::load_text(L, source.to_bytes(), source) };
    if rc != LUA_OK {
        // Classify while the message is still on the stack: once popped, the
        // string is unanchored and its bytes may be collected or the slot
        // reused by the next push.
        // SAFETY: the failed load left its error message on top.
        if continuation && unsafe { is_incomplete(L) } {
            // SAFETY: drops that message; nothing borrows it any more.
            unsafe { crate::ffi::lua_pop(L, 1) };
            return Outcome::Incomplete;
        }
        // SAFETY: the load error is still on top.
        unsafe { print_error(L) };
        return Outcome::Failed;
    }

    // lua_pcall(L,0,MULTRET,0) == lua_pcallk(L,0,MULTRET,0, 0, NULL)
    // SAFETY: L is valid and the compiled chunk is on top.
    let rc = unsafe {
        crate::ffi::lua_pcallk(L, 0, crate::ffi::LUA_MULTRET, 0, 0, core::ptr::null_mut())
    };
    if rc != LUA_OK {
        // SAFETY: the failed call left its error object on top.
        unsafe { print_error(L) };
        return Outcome::Failed;
    }
    Outcome::Ok
}

/// Check if the compile error at the stack top marks an incomplete chunk.
///
/// Reads the message in place and does not pop it; the borrowed bytes are
/// only valid while the message stays on the stack.
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
    // SAFETY: ptr points to `len` valid bytes of the string still on the stack.
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
    // SAFETY: L is valid and the error is on top. Pop only now: `bytes`
    // borrowed the message, which the pop unanchors.
    unsafe { crate::ffi::lua_settop(L, -2) };
}

#[cfg(test)]
#[path = "repl_session_tests.rs"]
mod tests;
