//! Lua 5.4 C FFI bindings.
//!
//! Only the minimal subset needed by the ViCell Lua cell.  See lua.h and
//! lauxlib.h in `src/c/src/` for the full API.
#![allow(dead_code)] // reason: FFI binding surface — keep the coherent lua.h subset even where bindings don't call every shim

use core::ffi::{c_char, c_int, CStr};

/// Opaque Lua interpreter state.  Always heap-allocated by Lua; never moved.
#[repr(C)]
pub struct LuaState {
    _opaque: [u8; 0],
}

pub const LUA_OK: c_int = 0;
pub const LUA_MULTRET: c_int = -1;

extern "C" {
    // ── Lifecycle ────────────────────────────────────────────────────────────

    /// Create a new Lua state using Lua's default allocator. That allocator
    /// uses the C malloc family, whose heap is backed by the glue's
    /// `__wrap__sbrk` static arena (the toolchain `_sbrk` stub returns null).
    pub fn luaL_newstate() -> *mut LuaState;

    /// Open all standard libraries into `L`.
    pub fn luaL_openlibs(L: *mut LuaState);

    /// Close and free a Lua state.
    pub fn lua_close(L: *mut LuaState);

    // ── Execution ────────────────────────────────────────────────────────────

    /// Compile `sz` bytes at `buff` as a Lua chunk named `name`.
    ///
    /// Private on purpose: callers go through [`load_text`], which pins
    /// `mode` to text. A null or "b"-containing mode would let the
    /// unverified bytecode undumper run on attacker bytes.
    fn luaL_loadbufferx(
        L: *mut LuaState,
        buff: *const c_char,
        sz: usize,
        name: *const c_char,
        mode: *const c_char,
    ) -> c_int;

    /// Call the function at the top of the stack with `nargs` arguments.
    /// `LUA_MULTRET` for `nresults` accepts any number of results.
    ///
    /// Note: `lua_pcall` is a macro in lua.h that forwards to `lua_pcallk`
    /// with no continuation; we bind the real `lua_pcallk` symbol directly.
    /// `ctx`/`k` are the continuation context/function — pass 0/null for a
    /// plain protected call.
    pub fn lua_pcallk(
        L: *mut LuaState,
        nargs: c_int,
        nresults: c_int,
        errfunc: c_int,
        ctx: isize,
        k: *mut core::ffi::c_void,
    ) -> c_int;

    /// Raise the value at the stack top as a Lua error.
    ///
    /// Never returns: it longjmps to the innermost protected call, skipping
    /// every Rust frame in between without running destructors. Use
    /// [`raise_or_return`] from a frame that owns nothing.
    pub fn lua_error(L: *mut LuaState) -> !;

    // ── Stack inspection ─────────────────────────────────────────────────────

    /// Return the string at stack index `idx` (negative = from top).
    /// Sets `*len` to the byte length.  Returns NULL if not a string.
    pub fn lua_tolstring(L: *mut LuaState, idx: c_int, len: *mut usize) -> *const c_char;

    /// Return the number of elements on the stack.
    pub fn lua_gettop(L: *mut LuaState) -> c_int;

    /// Pop `n` elements from the stack.
    pub fn lua_settop(L: *mut LuaState, idx: c_int);

    // ── Stack push ───────────────────────────────────────────────────────────

    /// Push a nil value.
    pub fn lua_pushnil(L: *mut LuaState);
    /// Push a boolean (0 = false, non-zero = true).
    pub fn lua_pushboolean(L: *mut LuaState, b: c_int);
    /// Push a 64-bit integer.
    pub fn lua_pushinteger(L: *mut LuaState, n: i64);
    /// Push a NUL-terminated C string; returns the internal pointer.
    pub fn lua_pushstring(L: *mut LuaState, s: *const c_char) -> *const c_char;
    /// Push `len` bytes of string data (may contain NUL bytes).
    pub fn lua_pushlstring(L: *mut LuaState, s: *const c_char, len: usize) -> *const c_char;
    /// Push a light userdata (opaque pointer, not GC-managed).
    pub fn lua_pushlightuserdata(L: *mut LuaState, p: *mut core::ffi::c_void);

    // ── Stack read ───────────────────────────────────────────────────────────

    /// Return the integer at stack index `idx` (0 if not a number).
    pub fn lua_tointegerx(L: *mut LuaState, idx: c_int, isnum: *mut c_int) -> i64;
    /// Return the light userdata pointer at stack index `idx` (null if wrong type).
    pub fn lua_touserdata(L: *mut LuaState, idx: c_int) -> *mut core::ffi::c_void;

    // ── Function registration / tables ───────────────────────────────────────

    /// Push a C closure with `n` upvalues.
    ///
    /// `lua_pushcfunction(L,f)` in lua.h is the macro `lua_pushcclosure(L,f,0)`;
    /// bind the real exported symbol and pass `n=0` for a plain C function.
    pub fn lua_pushcclosure(
        L: *mut LuaState,
        f: unsafe extern "C" fn(*mut LuaState) -> c_int,
        n: c_int,
    );

    /// Pop the value at stack top and set it as the global `name`.
    pub fn lua_setglobal(L: *mut LuaState, name: *const c_char);

    /// Create a new empty table and push it onto the stack.
    ///
    /// `lua_newtable(L)` in lua.h is the macro `lua_createtable(L,0,0)`.
    pub fn lua_createtable(L: *mut LuaState, narr: c_int, nrec: c_int);

    /// `t[k] = v` where `t` is at stack index `idx` and `v` is at the top
    /// (popped). `k` is a NUL-terminated C string.
    pub fn lua_setfield(L: *mut LuaState, idx: c_int, k: *const c_char);

    /// `t[n] = v` where `t` is the table at `idx` and `v` is at the top (popped).
    ///
    /// `n` is a `lua_Integer` (i64) key, used for 1-indexed array construction.
    /// Bypasses `__newindex` metamethod — use for raw table writes.
    pub fn lua_rawseti(L: *mut LuaState, idx: c_int, n: i64);
}

/// Compile `src` as Lua source text and push the chunk (or an error string).
///
/// The only chunk loader exposed to the cell. Mode "t" makes Lua refuse any
/// buffer starting with the bytecode signature `\x1bLua` before the undumper
/// sees it; the script-level `load`/`loadfile`/`dofile`/`require` are pinned
/// to the same mode by `sandbox-prelude.lua`. Returns `LUA_OK` on success.
///
/// # Safety
/// `L` must be a valid, non-null Lua state.
#[allow(non_snake_case)] // reason: L is the Lua C API convention for lua_State pointers
pub unsafe fn load_text(L: *mut LuaState, src: &[u8], name: &CStr) -> c_int {
    // SAFETY: caller guarantees L is valid; `src` is readable for its length
    // (no NUL terminator required); `name` and the "t" literal are NUL-terminated
    // and outlive the call.
    unsafe {
        luaL_loadbufferx(
            L,
            src.as_ptr().cast::<c_char>(),
            src.len(),
            name.as_ptr(),
            c"t".as_ptr(),
        )
    }
}

/// Convenience: pop `n` items from the stack.
///
/// # Safety
/// `L` must be a valid, non-null Lua state.
#[allow(non_snake_case)] // reason: L is the Lua C API convention for lua_State pointers
pub unsafe fn lua_pop(L: *mut LuaState, n: c_int) {
    // SAFETY: caller guarantees L is valid; settop(-n-1) is the canonical pop.
    unsafe { lua_settop(L, -(n) - 1) }
}

/// A protected call failed and left its error object on the stack top.
///
/// Bindings carry it out of the frames that own destructor-bearing values;
/// [`raise_or_return`] re-raises the error once those frames are gone.
pub struct LuaRaised;

/// Run `push` under `lua_pcall`, so a Lua error it raises (chiefly
/// `LUA_ERRMEM` while allocating a string or table) unwinds only the
/// trampoline instead of longjmp-ing over the caller and skipping the
/// destructors of the values it still owns (a leaked `Vec` per failed call).
///
/// On `Ok` the values `push` left are on the caller's stack; on `Err` the
/// error object is, and the caller must drop what it owns and then hand the
/// error to [`raise_or_return`]. Values `push` captures by reference live in
/// the shielded caller frame; `push` itself must not own a destructor-bearing
/// value across a Lua API call, because its own frame is still unwound.
///
/// # Safety
/// `L` must be the state of a running `lua_CFunction` (`LUA_MINSTACK` then
/// guarantees the two slots the call needs), and `push` must use only the
/// stack slots `LUA_MINSTACK` guarantees a fresh C function.
#[allow(non_snake_case)] // reason: L is the Lua C API convention for lua_State pointers
pub unsafe fn pcall_push<F: Fn(*mut LuaState)>(
    L: *mut LuaState,
    push: &F,
) -> Result<(), LuaRaised> {
    // SAFETY: L is valid per the contract. A zero-upvalue C closure and a light
    // userdata are stored inline and never allocate, so neither push can raise
    // outside the protected call. The pointer stays valid for the whole call:
    // `push` is borrowed by this frame, which blocks in lua_pcallk until the
    // trampoline has returned or unwound.
    let status = unsafe {
        lua_pushcclosure(L, push_trampoline::<F>, 0);
        lua_pushlightuserdata(L, core::ptr::from_ref(push).cast_mut().cast());
        lua_pcallk(L, 1, LUA_MULTRET, 0, 0, core::ptr::null_mut())
    };
    if status == LUA_OK {
        Ok(())
    } else {
        Err(LuaRaised)
    }
}

/// `lua_CFunction` run by [`pcall_push`]; argument 1 is a light userdata
/// pointing at the `F` its caller borrows. Returns every value `F` pushed.
///
/// # Safety
/// Only called by Lua from [`pcall_push`], with that light userdata as the
/// sole argument.
#[allow(non_snake_case)] // reason: L is the Lua C API convention for lua_State pointers
unsafe extern "C" fn push_trampoline<F: Fn(*mut LuaState)>(L: *mut LuaState) -> c_int {
    // SAFETY: argument 1 was pushed by pcall_push from a `&F` that outlives
    // this call (see there). Clearing the frame leaves `push` an empty stack.
    unsafe {
        let push = &*lua_touserdata(L, 1).cast::<F>();
        lua_settop(L, 0);
        push(L);
        lua_gettop(L)
    }
}

/// Return `nresults` from a binding, or re-raise the error a [`pcall_push`]
/// left on the stack top.
///
/// Call it in the binding's outermost frame on the result of an inner
/// function that owned the binding's destructor-bearing values: by then they
/// are dropped, so the `lua_error` longjmp skips nothing. A memory error keeps
/// its `LUA_ERRMEM` status (`lua_error` recognises the preallocated message).
///
/// # Safety
/// `L` must be the state of the running `lua_CFunction`; on `Err` its stack
/// top must still be the error object `pcall_push` left.
#[allow(non_snake_case)] // reason: L is the Lua C API convention for lua_State pointers
pub unsafe fn raise_or_return(L: *mut LuaState, result: Result<c_int, LuaRaised>) -> c_int {
    match result {
        Ok(nresults) => nresults,
        // SAFETY: L is valid and the error object is on top (contract).
        Err(LuaRaised) => unsafe { lua_error(L) },
    }
}

#[cfg(test)]
#[path = "ffi_tests.rs"]
mod tests;
