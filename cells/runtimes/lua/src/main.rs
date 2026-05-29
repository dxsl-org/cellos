#![no_std]
#![no_main]

extern crate ostd;
extern crate api;

mod bindings_io;
mod ffi;
mod repl_session;

#[no_mangle]
#[allow(non_snake_case)] // reason: L is the Lua API convention
extern "C" fn main() -> usize {
    // SAFETY: luaL_newstate allocates a new Lua state via malloc;
    // the returned pointer is valid until lua_close is called.
    let L = unsafe { ffi::luaL_newstate() };
    if L.is_null() {
        ostd::io::println("[lua] out of memory");
        return 1;
    }

    // SAFETY: L is non-null; luaL_openlibs is safe to call once.
    unsafe { ffi::luaL_openlibs(L) };

    // Enhanced REPL: multi-line support, history, Ctrl+C/D.
    ostd::io::println("Lua 5.4 on ViOS  (Ctrl+D to exit)");
    // SAFETY: L is non-null and valid; run_repl drives the full REPL loop.
    unsafe { repl_session::run_repl(L); }

    // SAFETY: L is non-null; lua_close frees the state.
    unsafe { ffi::lua_close(L) };
    0
}
