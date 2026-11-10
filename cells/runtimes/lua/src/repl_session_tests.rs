//! Host tests for the REPL chunk classification. They stay on the paths that
//! print nothing: `ostd::io` issues a CellOS syscall, which a host process
//! must not execute.

use super::*;
use crate::ffi;

/// A fresh Lua state closed on drop.
struct State(*mut LuaState);

impl State {
    fn new() -> Self {
        // SAFETY: luaL_newstate has no preconditions; null means OOM.
        let state = unsafe { ffi::luaL_newstate() };
        assert!(!state.is_null(), "luaL_newstate failed");
        Self(state)
    }

    fn top(&self) -> core::ffi::c_int {
        // SAFETY: self.0 is a live state.
        unsafe { ffi::lua_gettop(self.0) }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        // SAFETY: self.0 came from luaL_newstate and is closed exactly once.
        unsafe { ffi::lua_close(self.0) }
    }
}

/// Push `bytes` as a Lua string.
fn push_str(state: &State, bytes: &[u8]) {
    // SAFETY: self.0 is live; `bytes` is readable for its length.
    unsafe { ffi::lua_pushlstring(state.0, bytes.as_ptr().cast(), bytes.len()) };
}

#[test]
fn incomplete_chunk_is_classified_from_its_own_message() {
    let state = State::new();
    // A value below the message that would read as complete: the old code
    // popped the message first and then classified whatever lay beneath it.
    push_str(&state, b"below");

    // SAFETY: state is live.
    let outcome = unsafe { try_eval(state.0, "for i = 1, 2 do", true) };

    assert_eq!(outcome, Outcome::Incomplete);
    // The message was popped, the value below it left alone.
    assert_eq!(state.top(), 1);
}

#[test]
fn continuation_lines_complete_the_chunk() {
    let state = State::new();

    // SAFETY: state is live.
    unsafe {
        assert_eq!(try_eval(state.0, "x = {", true), Outcome::Incomplete);
        assert_eq!(try_eval(state.0, "x = {\n 41 + 1 }", true), Outcome::Ok);
        assert_eq!(try_eval(state.0, "return x[1]", true), Outcome::Ok);
    }
    let mut valid = 0;
    // SAFETY: state is live; the chunk's single result is on top.
    let value = unsafe { ffi::lua_tointegerx(state.0, -1, &mut valid) };
    assert_eq!((valid, value), (1, 42));
}

#[test]
fn is_incomplete_reads_the_message_in_place() {
    let state = State::new();
    for (source, incomplete) in [(&b"if true then"[..], true), (b"x = = 1", false)] {
        // SAFETY: state is live; `source` and the name outlive the call.
        let rc = unsafe { ffi::load_text(state.0, source, c"=test") };
        assert_ne!(rc, LUA_OK);
        // SAFETY: the failed load left its message on top.
        assert_eq!(unsafe { is_incomplete(state.0) }, incomplete);
        // Classification does not pop: the caller still owns the message.
        assert_eq!(state.top(), 1);
        // SAFETY: state is live; drops the message.
        unsafe { ffi::lua_pop(state.0, 1) };
    }
}
