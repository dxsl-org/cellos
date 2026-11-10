//! Host tests for [`pcall_push`] / [`raise_or_return`]: a Lua error raised
//! while a binding owns a destructor-bearing value must still run that
//! destructor, and must still reach the caller as `LUA_ERRMEM`.

use super::*;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const LUA_ERRMEM: c_int = 4;
const PAYLOAD_LEN: usize = 4096;

type LuaAlloc = unsafe extern "C" fn(*mut c_void, *mut c_void, usize, usize) -> *mut c_void;

extern "C" {
    fn lua_newstate(f: LuaAlloc, ud: *mut c_void) -> *mut LuaState;
    fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

/// Shared between the test, the allocator (as its `ud`) and the binding (as
/// its light userdata argument).
struct Ctx {
    /// Whether the binding switches the allocator to failing mode.
    fail_push: bool,
    /// While set, every allocation or growth fails (frees and shrinks pass).
    failing: AtomicBool,
    /// How many [`DropCounter`]s have been dropped.
    drops: AtomicUsize,
}

/// `lua_Alloc` over the C heap that fails on demand.
unsafe extern "C" fn failing_alloc(
    ud: *mut c_void,
    ptr: *mut c_void,
    osize: usize,
    nsize: usize,
) -> *mut c_void {
    // SAFETY: `ud` is the Ctx the test passed to lua_newstate; it outlives the state.
    let ctx = unsafe { &*ud.cast::<Ctx>() };
    if nsize == 0 {
        // SAFETY: Lua frees only blocks this allocator returned.
        unsafe { free(ptr) };
        return core::ptr::null_mut();
    }
    // `osize` is a type tag when `ptr` is null, so test the pointer first.
    if ctx.failing.load(Ordering::Relaxed) && (ptr.is_null() || nsize > osize) {
        return core::ptr::null_mut();
    }
    // SAFETY: `ptr` is null or a block this allocator returned.
    unsafe { realloc(ptr, nsize) }
}

/// A destructor whose run the test can observe.
struct DropCounter<'a>(&'a AtomicUsize);

impl Drop for DropCounter<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

/// A binding shaped like `vfs.read`: the body owns a buffer and a guard while
/// it pushes a string, and the outer frame re-raises.
#[allow(non_snake_case)] // reason: L is the Lua C API convention for lua_State pointers
unsafe extern "C" fn owning_binding(L: *mut LuaState) -> c_int {
    // SAFETY: L is the running state.
    unsafe { raise_or_return(L, owning_body(L)) }
}

#[allow(non_snake_case)] // reason: L is the Lua C API convention for lua_State pointers
unsafe fn owning_body(L: *mut LuaState) -> Result<c_int, LuaRaised> {
    // SAFETY: argument 1 is the test's light userdata pointing at a live Ctx.
    let ctx = unsafe { &*lua_touserdata(L, 1).cast::<Ctx>() };
    let _guard = DropCounter(&ctx.drops);
    let payload = alloc::vec![b'x'; PAYLOAD_LEN];
    ctx.failing.store(ctx.fail_push, Ordering::Relaxed);
    // SAFETY: L is the running state; the closure owns nothing.
    unsafe {
        pcall_push(L, &|L| {
            lua_pushlstring(L, payload.as_ptr().cast(), payload.len());
        })
    }?;
    Ok(1)
}

/// Call `owning_binding` under `lua_pcall` and return its status and result
/// string length, with the drop count.
fn run(fail_push: bool) -> (c_int, usize, usize) {
    let ctx = Ctx {
        fail_push,
        failing: AtomicBool::new(false),
        drops: AtomicUsize::new(0),
    };
    let ctx_ptr = core::ptr::from_ref(&ctx).cast_mut().cast::<c_void>();
    // SAFETY: `failing_alloc` honours the lua_Alloc contract; ctx outlives the state.
    let state = unsafe { lua_newstate(failing_alloc, ctx_ptr) };
    assert!(!state.is_null(), "lua_newstate failed");
    let mut len = 0;
    // SAFETY: state is live; the binding receives the Ctx pointer as argument 1.
    let status = unsafe {
        lua_pushcclosure(state, owning_binding, 0);
        lua_pushlightuserdata(state, ctx_ptr);
        let status = lua_pcallk(state, 1, 1, 0, 0, core::ptr::null_mut());
        ctx.failing.store(false, Ordering::Relaxed);
        if status == LUA_OK {
            lua_tolstring(state, -1, &mut len);
        }
        lua_close(state);
        status
    };
    (status, len, ctx.drops.load(Ordering::Relaxed))
}

#[test]
fn push_failure_drops_owned_values_and_reraises_errmem() {
    let (status, _, drops) = run(true);
    assert_eq!(status, LUA_ERRMEM);
    // The guard (and the payload beside it) were dropped before lua_error
    // unwound the binding; an unprotected push would leave this at 0.
    assert_eq!(drops, 1);
}

#[test]
fn push_success_returns_the_pushed_value() {
    assert_eq!(run(false), (LUA_OK, PAYLOAD_LEN, 1));
}
