//! Tetris-Lua cell — Rust platform host.
//!
//! Exposes three Lua global tables to the embedded tetris.lua script:
//!   surface.fill_rect(x,y,w,h,ci)  surface.print(x,y,text,ci,scale)
//!   surface.flush()  surface.width()  surface.height()
//!   input.poll_key()  — 0=none 1=left 2=right 3=rotate 4=down 5=drop 6=quit
//!   time.ticks()     — milliseconds

#![no_std]
#![no_main]
#![allow(unsafe_code)]
#![allow(static_mut_refs)]

// Links the Tier-A C library (libc symbols) the C objects call.
extern crate alloc;
extern crate posix_shim;

use abi::declare_manifest;

// All implementation-specific imports are gated so the fallback stub compiles
// cleanly when no ELF C compiler is available (lua_c_unavailable).
#[cfg(not(lua_c_unavailable))]
use core::ffi::{c_char, c_int};
#[cfg(not(lua_c_unavailable))]
use ostd::display::{linear::icons::LineIcon, wait_for_compositor, ViSurface};
#[cfg(not(lua_c_unavailable))]
use ostd::font::FONT8X8;
#[cfg(not(lua_c_unavailable))]
use ostd::input::{poll_events, request_focus};
#[cfg(not(lua_c_unavailable))]
use ostd::syscall::{sys_exit, sys_get_time};
#[cfg(not(lua_c_unavailable))]
use ostd::task::yield_now;
#[cfg(not(lua_c_unavailable))]
use wire::display::PixelFormat;
#[cfg(not(lua_c_unavailable))]
use wire::input::{InputEvent, KeyState, KeySym};

declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = abi::manifest::PROTECTION_CLASS_FFI
);

#[cfg(not(lua_c_unavailable))]
const SURF_W: u32 = wire::display::FALLBACK_WIDTH;
#[cfg(not(lua_c_unavailable))]
const SURF_H: u32 = wire::display::FALLBACK_HEIGHT;

// ── Full implementation (requires Lua C library) ──────────────────────────────

// CGA 16-color BGRA palette — indices 0-15 (A=FF, format 0xAARRGGBB).
#[cfg(not(lua_c_unavailable))]
static PALETTE: [u32; 16] = [
    0xFF000000, 0xFF0000AA, 0xFF00AA00, 0xFF00AAAA, 0xFFAA0000, 0xFFAA00AA, 0xFFAA5500, 0xFFAAAAAA,
    0xFF555555, 0xFF5555FF, 0xFF55FF55, 0xFF55FFFF, 0xFFFF5555, 0xFFFF55FF, 0xFFFFFF55, 0xFFFFFFFF,
];

#[cfg(not(lua_c_unavailable))]
static mut SURFACE: Option<ViSurface> = None;
#[cfg(not(lua_c_unavailable))]
static mut KEY_QUEUE: [u8; 16] = [0; 16];
#[cfg(not(lua_c_unavailable))]
static mut KEY_HEAD: usize = 0;
#[cfg(not(lua_c_unavailable))]
static mut KEY_TAIL: usize = 0;

// ── Input ─────────────────────────────────────────────────────────────────────

#[cfg(not(lua_c_unavailable))]
unsafe fn enqueue(k: u8) {
    let next = (KEY_TAIL + 1) % 16;
    if next != KEY_HEAD {
        KEY_QUEUE[KEY_TAIL] = k;
        KEY_TAIL = next;
    }
}

#[cfg(not(lua_c_unavailable))]
fn keysym_to_code(sym: KeySym) -> u8 {
    match sym {
        KeySym::Left => 1,
        KeySym::Right => 2,
        KeySym::Up => 3,
        KeySym::Down => 4,
        KeySym::Return => 5,
        KeySym::Escape => 6,
        _ => 0,
    }
}

// ── Pixel helpers (called by Lua C functions) ─────────────────────────────────

#[cfg(not(lua_c_unavailable))]
unsafe fn fill_rect_px(x: i32, y: i32, w: i32, h: i32, bgra: u32) {
    let s = match SURFACE.as_mut() {
        Some(s) => s,
        None => return,
    };
    let stride = s.stride();
    let sw = s.width() as i32;
    let sh = s.height() as i32;
    let pixels = s.pixels_mut();
    let [b, g, r, a] = bgra.to_le_bytes();
    // Script coordinates are arbitrary: clamp each edge into the surface in i32
    // so a negative or overflowing edge yields an empty range, never a wrapped usize.
    let y0 = y.clamp(0, sh) as usize;
    let y1 = y.saturating_add(h).clamp(0, sh) as usize;
    let x0 = x.clamp(0, sw) as usize;
    let x1 = x.saturating_add(w).clamp(0, sw) as usize;
    for py in y0..y1 {
        for px in x0..x1 {
            let o = py * stride + px * 4;
            pixels[o] = b;
            pixels[o + 1] = g;
            pixels[o + 2] = r;
            pixels[o + 3] = a;
        }
    }
}

/// `scale` must be positive; each set font bit becomes one clipped
/// `scale` x `scale` rectangle.
#[cfg(not(lua_c_unavailable))]
unsafe fn draw_glyph(px: i32, py: i32, c: u8, bgra: u32, scale: i32) {
    let idx = if (0x20..=0x7E).contains(&c) {
        (c - 0x20) as usize
    } else {
        0
    };
    for (row, &mask) in (0i32..).zip(FONT8X8[idx].iter()) {
        for col in 0..8i32 {
            if mask & (0x80u8 >> col as u32) == 0 {
                continue;
            }
            fill_rect_px(
                px.saturating_add(col.saturating_mul(scale)),
                py.saturating_add(row.saturating_mul(scale)),
                scale,
                scale,
                bgra,
            );
        }
    }
}

// ── Lua C callbacks ───────────────────────────────────────────────────────────
#[cfg(not(lua_c_unavailable))]
#[allow(non_snake_case)]
mod lua_fns {
    use super::*;
    use crate::ffi::*;

    pub unsafe extern "C" fn fill_rect(L: *mut LuaState) -> c_int {
        let x = lua_tointegerx(L, 1, core::ptr::null_mut()) as i32;
        let y = lua_tointegerx(L, 2, core::ptr::null_mut()) as i32;
        let w = lua_tointegerx(L, 3, core::ptr::null_mut()) as i32;
        let h = lua_tointegerx(L, 4, core::ptr::null_mut()) as i32;
        let ci = lua_tointegerx(L, 5, core::ptr::null_mut()) as usize;
        fill_rect_px(x, y, w, h, PALETTE[ci.min(15)]);
        0
    }

    pub unsafe extern "C" fn print_text(L: *mut LuaState) -> c_int {
        let x = lua_tointegerx(L, 1, core::ptr::null_mut()) as i32;
        let y = lua_tointegerx(L, 2, core::ptr::null_mut()) as i32;
        let mut n = 0usize;
        let ptr = lua_tolstring(L, 3, &mut n);
        let ci = lua_tointegerx(L, 4, core::ptr::null_mut()) as usize;
        let scale = lua_tointegerx(L, 5, core::ptr::null_mut()).clamp(1, i32::MAX as i64) as i32;
        if !ptr.is_null() {
            let text = core::slice::from_raw_parts(ptr as *const u8, n);
            let color = PALETTE[ci.min(15)];
            let advance = scale.saturating_mul(8);
            let mut gx = x;
            for &c in text {
                draw_glyph(gx, y, c, color, scale);
                gx = gx.saturating_add(advance);
            }
        }
        0
    }

    pub unsafe extern "C" fn flush(_L: *mut LuaState) -> c_int {
        if let Some(ref s) = SURFACE {
            s.damage_all();
        }
        0
    }

    pub unsafe extern "C" fn width(L: *mut LuaState) -> c_int {
        lua_pushinteger(L, SURF_W as i64);
        1
    }

    pub unsafe extern "C" fn height(L: *mut LuaState) -> c_int {
        lua_pushinteger(L, SURF_H as i64);
        1
    }

    pub unsafe extern "C" fn poll_key(L: *mut LuaState) -> c_int {
        let evs = poll_events(8);
        for ev in evs {
            if let InputEvent::Key(ke) = ev {
                match ke.state {
                    KeyState::Pressed | KeyState::Repeated => {
                        // Space bar comes through as Printable(' ')
                        let k = if ke.char() == Some(' ') {
                            5
                        } else {
                            keysym_to_code(ke.keysym)
                        };
                        if k != 0 {
                            enqueue(k);
                        }
                    }
                    _ => {}
                }
            }
        }
        let k = if KEY_HEAD == KEY_TAIL {
            0
        } else {
            let v = KEY_QUEUE[KEY_HEAD];
            KEY_HEAD = (KEY_HEAD + 1) % 16;
            v
        };
        lua_pushinteger(L, k as i64);
        1
    }

    pub unsafe extern "C" fn ticks(L: *mut LuaState) -> c_int {
        lua_pushinteger(L, (sys_get_time() / 10_000) as i64);
        1
    }
}

// ── Embedded Lua script ───────────────────────────────────────────────────────

#[cfg(not(lua_c_unavailable))]
const TETRIS_LUA: &[u8] = include_bytes!("../scripts/tetris.lua");

// ── Entry ─────────────────────────────────────────────────────────────────────

#[cfg(lua_c_unavailable)]
#[no_mangle]
extern "C" fn main() -> usize {
    ostd::io::println("[tetris-lua] not available: no ELF C compiler at build time.");
    1
}

#[cfg(not(lua_c_unavailable))]
mod ffi;

#[cfg(not(lua_c_unavailable))]
#[no_mangle]
#[allow(non_snake_case)]
extern "C" fn main() -> usize {
    use ffi::*;

    let comp = wait_for_compositor();
    unsafe {
        match ViSurface::create(comp, SURF_W, SURF_H, PixelFormat::Bgra8888) {
            Ok(s) => {
                if s.set_icon(LineIcon::Blocks).is_err() {
                    ostd::io::println(
                        "[tetris-lua] set_icon failed; Cell Tab keeps the generic icon",
                    );
                }
                s.raise();
                SURFACE = Some(s);
            }
            Err(_) => {
                sys_exit(1);
            }
        }
        while !request_focus() {
            yield_now();
        }

        let L = luaL_newstate();
        if L.is_null() {
            sys_exit(1);
        }
        luaL_openlibs(L);

        // Register `surface` table
        lua_createtable(L, 0, 5);
        macro_rules! reg {
            ($fn:expr, $name:expr) => {
                lua_pushcclosure(L, $fn, 0);
                lua_setfield(L, -2, $name.as_ptr());
            };
        }
        reg!(lua_fns::fill_rect, c"fill_rect");
        reg!(lua_fns::print_text, c"print");
        reg!(lua_fns::flush, c"flush");
        reg!(lua_fns::width, c"width");
        reg!(lua_fns::height, c"height");
        lua_setglobal(L, c"surface".as_ptr());

        // Register `input` table
        lua_createtable(L, 0, 1);
        reg!(lua_fns::poll_key, c"poll_key");
        lua_setglobal(L, c"input".as_ptr());

        // Register `time` table
        lua_createtable(L, 0, 1);
        reg!(lua_fns::ticks, c"ticks");
        lua_setglobal(L, c"time".as_ptr());

        // Load and run the embedded script
        let rc = luaL_loadbufferx(
            L,
            TETRIS_LUA.as_ptr() as *const c_char,
            TETRIS_LUA.len(),
            c"tetris.lua".as_ptr(),
            core::ptr::null(),
        );
        if rc == LUA_OK {
            lua_pcallk(L, 0, LUA_MULTRET, 0, 0, core::ptr::null_mut());
        } else {
            let mut len = 0usize;
            let ptr = lua_tolstring(L, -1, &mut len);
            if !ptr.is_null() {
                let s = core::slice::from_raw_parts(ptr as *const u8, len);
                if let Ok(msg) = core::str::from_utf8(s) {
                    ostd::io::println(msg);
                }
            }
        }
    }
    loop {
        yield_now();
    }
}
