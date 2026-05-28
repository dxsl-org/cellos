#![no_std]
#![no_main]

extern crate ostd;
extern crate api;

use core::ffi::{c_void, c_char};

// --- FFI Bindings to Lua ---
#[repr(C)]
pub struct lua_State { _private: [u8; 0] }

const LUA_OK: i32 = 0;
const LUA_MULTRET: i32 = -1;
const LUA_GCSTOP: i32 = 0;
const LUA_GCRESTART: i32 = 1;

#[allow(dead_code)]
extern "C" {
    // lua.h
    fn lua_newstate(f: extern "C" fn(*mut c_void, *mut c_void, usize, usize) -> *mut c_void, ud: *mut c_void) -> *mut lua_State;
    fn lua_close(L: *mut lua_State);
    fn lua_settop(L: *mut lua_State, idx: i32);
    fn lua_pushstring(L: *mut lua_State, s: *const c_char) -> *const c_char;
    fn lua_tolstring(L: *mut lua_State, idx: i32, len: *mut usize) -> *const c_char;
    fn lua_pcallk(L: *mut lua_State, nargs: i32, nresults: i32, errfunc: i32, ctx: isize, k: Option<extern "C" fn(*mut lua_State, i32, isize) -> i32>) -> i32;
    fn lua_gc(L: *mut lua_State, what: i32, ...) -> i32;

    // lauxlib.h
    fn luaL_newstate() -> *mut lua_State;
    fn luaL_openlibs(L: *mut lua_State);
    fn luaL_loadbufferx(L: *mut lua_State, buff: *const c_char, sz: usize, name: *const c_char, mode: *const c_char) -> i32;

    // Libc init
    fn __libc_init_array();
    
    // Vi shim init
    fn init_impure_ptr();
}

#[allow(non_snake_case)]
unsafe fn lua_pop(L: *mut lua_State, n: i32) {
    lua_settop(L, -(n)-1);
}

#[allow(non_snake_case)]
unsafe fn lua_pcall(L: *mut lua_State, n: i32, r: i32, f: i32) -> i32 {
    lua_pcallk(L, n, r, f, 0, None)
}

#[allow(non_snake_case)]
unsafe fn lua_tostring(L: *mut lua_State, i: i32) -> &'static str {
    let mut len = 0;
    let ptr = lua_tolstring(L, i, &mut len);
    if ptr.is_null() { return ""; }
    let slice = core::slice::from_raw_parts(ptr as *const u8, len);
    core::str::from_utf8_unchecked(slice)
}

// Ensure libc initialization symbols exist
#[no_mangle]
pub unsafe extern "C" fn _init() {}

#[no_mangle]
pub unsafe extern "C" fn _fini() {}

// --- Main REPL Logic ---

#[no_mangle]
extern "C" fn main() -> usize {
    extern "C" {
        fn init_stdio_files();
    }

    unsafe {
        init_impure_ptr();
        __libc_init_array();
        init_stdio_files();
        
        // TEST: Direct Write to FD 1
        // let msg = "DEBUG: Direct Write Test (FD 1)\n";
        // api::posix::_write(1, msg.as_ptr() as *const c_void, msg.len());
    }

    ostd::io::print("Lua 5.4.7 ViOS DEBUG 3\n");

    unsafe {
        ostd::io::print("DEBUG: Calling luaL_newstate...\n"); 
        let l = luaL_newstate();
        if l.is_null() {
                ostd::io::print("Error: Cannot create state: not enough memory\n");
                return 1;
            }
            ostd::io::print("DEBUG: luaL_newstate success.\n");

            ostd::io::print("DEBUG: Calling lua_gc STOP...\n");
            lua_gc(l, LUA_GCSTOP, 0); 
            ostd::io::print("DEBUG: lua_gc STOP success.\n");

            ostd::io::print("DEBUG: Calling luaL_openlibs...\n");
            luaL_openlibs(l); 
            ostd::io::print("DEBUG: luaL_openlibs success.\n");
            
            ostd::io::print("DEBUG: Calling lua_gc RESTART...\n");
            lua_gc(l, LUA_GCRESTART, 0);
            ostd::io::print("DEBUG: lua_gc RESTART success.\n");

            ostd::io::print("Interactive mode ready. Type 'exit' to quit.\n");
            
            loop {
                ostd::io::print("> ");
            
            let mut raw_buf = [0u8; 512];
            let mut i = 0;
            // Read loop: consume stdin until newline
            loop {
                if i >= 511 { break; }
                let mut c = [0u8; 1];
                let n = api::posix::_read(0, c.as_mut_ptr() as *mut c_void, 1);
                if n <= 0 { break; }
                raw_buf[i] = c[0];
                i += 1;
                if c[0] == b'\n' { break; }
            }
            
            if i == 0 { break; }
            
            // Null terminate
            raw_buf[i] = 0;
            let ptr = raw_buf.as_ptr() as *const c_char;
            
            // Remove trailing newline for exit check and Lua buffer
            if i > 0 && raw_buf[i-1] == b'\n' {
                raw_buf[i-1] = 0;
            }

            let cmd_str = core::str::from_utf8(&raw_buf[..i]).unwrap_or("");
            let cmd_trim = cmd_str.trim_end(); // Trims \n\r spaces
            
            if cmd_trim.is_empty() {
                continue;
            }

            if cmd_trim == "exit" {
                break;
            }

            // Load string
            let chunk_name = b"=stdin\0";
            let status = luaL_loadbufferx(l, ptr, cmd_trim.len(), chunk_name.as_ptr() as *const c_char, core::ptr::null());
            
            if status != LUA_OK {
                print_error(l);
                continue;
            }

            // Call
            let status = lua_pcall(l, 0, LUA_MULTRET, 0);
             if status != LUA_OK {
                print_error(l);
            }
        }

        lua_close(l);
    }

    0
}

unsafe fn print_error(l: *mut lua_State) {
    let msg = lua_tostring(l, -1);
    ostd::io::print("lua: ");
    ostd::io::print(msg);
    ostd::io::print("\n");
    lua_pop(l, 1);
}
