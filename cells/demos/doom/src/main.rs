//! DOOM port for ViCell — doomgeneric platform backend.
//!
//! Implements the 6 doomgeneric platform hooks as `#[no_mangle] extern "C"`
//! functions, wiring DOOM's rendering and input into the ViCell compositor
//! and input service.
//!
//! ## How to run
//! 1. Clone doomgeneric: `git clone https://github.com/ozkl/doomgeneric cells/games/doom/src/c/doomgeneric`
//! 2. Place `doom1.wad` (shareware) on the ViCell FAT32 disk as `/doom1.wad`
//! 3. Build and spawn: the compositor must be running; keyboard focus is requested at init.

#![no_std]
#![no_main]
#![allow(unsafe_code)]
#![allow(static_mut_refs)] // single-task cell — no data race on SURFACE / KEY_QUEUE
#![allow(clippy::missing_safety_doc)] // reason: DG_* hooks are C callbacks; their contract lives in doomgeneric's headers, all single-task

// Links the Tier-A C library (libc symbols) the C objects call.
extern crate alloc;
extern crate posix_shim;

use abi::declare_manifest;
use alloc::vec::Vec;
use ostd::display::{linear::icons::LineIcon, wait_for_compositor, ViSurface};
use ostd::input::{poll_events, request_focus};
use ostd::syscall::{sys_exit, sys_get_time};
use ostd::task::yield_now;
use wire::display::PixelFormat;
use wire::input::{InputEvent, KeyState, KeySym};

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

// DOOM internal framebuffer — must match DOOMGENERIC_RESX/RESY in build.rs.
const DOOM_W: u32 = 320;
const DOOM_H: u32 = 200;

// Compositor surface size: fill the whole screen. The compositor blits 1:1 with
// no scaling, so DOOM scales its own 320×200 framebuffer up into a screen-sized
// surface (nearest-neighbour). Stretching 320×200 → 1024×768 (both 4:3) yields
// DOOM's intended 4:3 aspect (its pixels are non-square by design).
const SCREEN_W: u32 = wire::display::FALLBACK_WIDTH; // 1024
const SCREEN_H: u32 = wire::display::FALLBACK_HEIGHT; // 768

// Source-column lookup (dst-x → src-x), filled once on the first frame so the
// per-pixel scale loop avoids a division per pixel.
static mut COLMAP: [u16; SCREEN_W as usize] = [0; SCREEN_W as usize];
static mut COLMAP_READY: bool = false;

declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = abi::manifest::PROTECTION_CLASS_FFI
);

/// Size of the static arena behind picolibc's `_sbrk` (10 MiB).
const DOOM_C_HEAP_SIZE: usize = 10 * 1024 * 1024;

/// Static C heap — serves picolibc malloc via `_sbrk`.
///
/// Without it, `_sbrk` (from sysio.rs) returns NULL → malloc always fails →
/// DOOM's Z_Init("couldn't allocate zone heap") exits immediately.
///
/// Interior-mutable instead of `static mut`: C keeps raw pointers into this
/// array for the life of the cell, so Rust must never form a reference (shared
/// or `&mut`) to the array — that would assert exclusive/frozen access over
/// bytes C is concurrently using. `UnsafeCell::get` yields a raw pointer with
/// no such claim, and it is the only way this module touches the arena.
#[repr(transparent)]
struct CHeap(UnsafeCell<[u8; DOOM_C_HEAP_SIZE]>);

// SAFETY: Rust never reads or writes the arena's bytes; it only computes raw
// pointers into it. All accesses happen in C through those pointers, on the
// cell's single task, so sharing the static across threads adds no data race.
unsafe impl Sync for CHeap {}

static DOOM_C_HEAP: CHeap = CHeap(UnsafeCell::new([0; DOOM_C_HEAP_SIZE]));

/// Current program break as a byte offset into [`DOOM_C_HEAP`], always in
/// `0..=DOOM_C_HEAP_SIZE`.
static DOOM_C_HEAP_BREAK: AtomicUsize = AtomicUsize::new(0);

/// `_sbrk` for picolibc malloc: moves the break by `incr` bytes and returns the
/// previous break, or `(void *)-1` (ENOMEM) when the new break would leave the
/// arena in either direction.
///
/// `--wrap=_sbrk` (doom/build.rs) redirects picolibc's `_sbrk` calls here.
/// Name convention: `__wrap_` + `_sbrk` = `__wrap__sbrk` (double underscore).
/// `--allow-multiple-definition` lets this win over sysio.rs's NULL stub.
#[no_mangle]
pub extern "C" fn __wrap__sbrk(incr: i32) -> *mut u8 {
    // checked_add_signed rejects a shrink past the arena start (including
    // incr == i32::MIN) instead of wrapping; the filter rejects growth past
    // the end. The atomic update keeps the break consistent even if a second
    // task ever called malloc, though DOOM runs on one task today.
    let moved = DOOM_C_HEAP_BREAK.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
        cur.checked_add_signed(incr as isize)
            .filter(|&new| new <= DOOM_C_HEAP_SIZE)
    });
    match moved {
        // `prev <= DOOM_C_HEAP_SIZE`, so the result stays inside the arena (or
        // one past its end) and keeps the arena's provenance; wrapping_add only
        // avoids an unsafe `add` whose bound is already checked above.
        Ok(prev) => DOOM_C_HEAP.0.get().cast::<u8>().wrapping_add(prev),
        Err(_) => core::ptr::without_provenance_mut(usize::MAX),
    }
}

// ─── doomgeneric public API ────────────────────────────────────────────────────

extern "C" {
    // Initialize DOOM engine (parses WAD, sets up renderer, calls DG_Init).
    fn doomgeneric_Create(argc: i32, argv: *const *const u8);
    // Run one game tick: input → game logic → DG_DrawFrame + DG_GetKey.
    fn doomgeneric_Tick();
}

// ─── Global state (set once in DG_Init, read in DG_DrawFrame / DG_GetKey) ────

static mut SURFACE: Option<ViSurface> = None;
// One-shot guard: log the first frame so headless runs can confirm rendering.
static mut FIRST_FRAME_LOGGED: bool = false;

// Ring buffer for pending DOOM key events: (pressed: bool, doomkey: u8)
static mut KEY_QUEUE: [KeyQueueEntry; 32] = [KeyQueueEntry {
    pressed: false,
    doomkey: 0,
}; 32];
static mut KEY_HEAD: usize = 0;
static mut KEY_TAIL: usize = 0;

#[derive(Clone, Copy)]
struct KeyQueueEntry {
    pressed: bool,
    doomkey: u8,
}

// ─── ViCell entry ──────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn main() {
    // Pass `-iwad /doom1.wad` so D_DoomMain can find the WAD on the ViFS.
    let iwad = c"-iwad";
    let wpath = c"/doom1.wad";
    let argv: [*const u8; 3] = [
        c"doom".as_ptr().cast(),
        iwad.as_ptr().cast(),
        wpath.as_ptr().cast(),
    ];

    // Initialize engine + start game loop (doomgeneric_Tick never returns).
    unsafe {
        doomgeneric_Create(3, argv.as_ptr());
        loop {
            doomgeneric_Tick();
        }
    }
}

// ─── DG_Init ──────────────────────────────────────────────────────────────────

/// Called once by doomgeneric before D_DoomMain (and therefore before W_Init).
/// Creates a 320×200 compositor surface and requests keyboard focus.
#[no_mangle]
pub unsafe extern "C" fn DG_Init() {
    ostd::io::println("DOOM: DG_Init — creating fullscreen surface");
    let comp_tid = wait_for_compositor();
    match ViSurface::create(comp_tid, SCREEN_W, SCREEN_H, PixelFormat::Bgra8888) {
        Ok(surf) => {
            if surf.set_icon(LineIcon::Blocks).is_err() {
                ostd::io::println("DOOM: set_icon failed; Cell Tab keeps the generic icon");
            }
            surf.raise(); // own the top of the z-order
            SURFACE = Some(surf);
        }
        Err(_) => {
            sys_exit(1);
        }
    }
    // Spin until input service is registered and grants focus.
    // During boot race this can take a few ticks.
    while !request_focus() {
        yield_now();
    }
}

// ─── DG_DrawFrame ─────────────────────────────────────────────────────────────

/// Called by doomgeneric after each rendered frame.
/// Scales the 320×200 BGRA framebuffer up to the screen-sized Grant surface
/// (nearest-neighbour) so DOOM fills the display.
///
/// doomgeneric stores screen data as 32-bit ARGB values (0xAARRGGBB).
/// In little-endian memory the bytes are [B, G, R, A] — matching Bgra8888,
/// so each 32-bit word copies verbatim with no channel swizzle.
#[no_mangle]
pub unsafe extern "C" fn DG_DrawFrame() {
    extern "C" {
        // DG_ScreenBuffer: allocated by doomgeneric_Create, 320×200 uint32_t.
        // C owns it and neither frees nor resizes it during this callback.
        static DG_ScreenBuffer: *mut u32;
    }
    let surf = match SURFACE.as_mut() {
        Some(s) => s,
        None => return,
    };
    let screen = DG_ScreenBuffer;
    if screen.is_null() {
        return;
    }

    // SAFETY: non-null, and C keeps the DOOM_W×DOOM_H buffer alive and
    // unmodified for the duration of this callback.
    let src = core::slice::from_raw_parts(screen, (DOOM_W * DOOM_H) as usize);
    // The &mut borrow from pixels_mut() ends at this statement; `dst` below is a
    // fresh slice over the same Grant pages, leaving `surf` free for damage_all().
    let dst_ptr = surf.pixels_mut().as_mut_ptr() as *mut u32;
    let dst = core::slice::from_raw_parts_mut(dst_ptr, (SCREEN_W * SCREEN_H) as usize);

    if !COLMAP_READY {
        for (dx, col) in COLMAP.iter_mut().enumerate().take(SCREEN_W as usize) {
            *col = (dx as u32 * DOOM_W / SCREEN_W) as u16;
        }
        COLMAP_READY = true;
    }

    for dy in 0..SCREEN_H as usize {
        let sy = (dy as u32 * DOOM_H / SCREEN_H) as usize;
        let src_row = &src[sy * DOOM_W as usize..(sy + 1) * DOOM_W as usize];
        let dst_row = &mut dst[dy * SCREEN_W as usize..(dy + 1) * SCREEN_W as usize];
        for (dst_px, &col) in dst_row.iter_mut().zip(COLMAP.iter()) {
            *dst_px = src_row[col as usize];
        }
    }

    surf.damage_all();
    if !FIRST_FRAME_LOGGED {
        FIRST_FRAME_LOGGED = true;
        ostd::io::println("DOOM: DG_DrawFrame — first frame rendered to compositor");
    }
}

// ─── DG_GetKey ────────────────────────────────────────────────────────────────

/// Non-blocking key poll consumed by doomgeneric's event loop.
/// Returns 1 if an event is available, 0 if the queue is empty.
/// `*pressed` = 1 for press, 0 for release. `*doomkey` = DOOM key code.
#[no_mangle]
pub unsafe extern "C" fn DG_GetKey(pressed: *mut i32, doomkey: *mut u8) -> i32 {
    // Drain any new InputEvents from the input service into our ring buffer.
    let events: Vec<InputEvent> = poll_events(16);
    for ev in events {
        if let InputEvent::Key(ke) = ev {
            let dk = keysym_to_doom(ke.keysym, ke.character);
            if dk == 0 {
                continue;
            }
            let p = ke.state == KeyState::Pressed || ke.state == KeyState::Repeated;
            let tail = KEY_TAIL;
            let next = (tail + 1) % 32;
            if next != KEY_HEAD {
                KEY_QUEUE[tail] = KeyQueueEntry {
                    pressed: p,
                    doomkey: dk,
                };
                KEY_TAIL = next;
            }
        }
    }

    if KEY_HEAD == KEY_TAIL {
        return 0;
    }
    let entry = KEY_QUEUE[KEY_HEAD];
    KEY_HEAD = (KEY_HEAD + 1) % 32;
    *pressed = entry.pressed as i32;
    *doomkey = entry.doomkey;
    1
}

// ─── DG_GetTicksMs ────────────────────────────────────────────────────────────

/// Monotonic millisecond counter.  sys_get_time() returns 10 MHz ticks.
#[no_mangle]
pub extern "C" fn DG_GetTicksMs() -> u32 {
    (sys_get_time() / 10_000) as u32
}

// ─── DG_SleepMs ───────────────────────────────────────────────────────────────

/// Busy-yield sleep.  Accurate enough for DOOM's 35 Hz game tick (~28 ms/frame).
#[no_mangle]
pub extern "C" fn DG_SleepMs(ms: u32) {
    let deadline = sys_get_time() + ms as u64 * 10_000;
    while sys_get_time() < deadline {
        yield_now();
    }
}

// ─── DG_Quit ──────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn DG_Quit() {
    sys_exit(0);
}

// ─── DG_SetWindowTitle ────────────────────────────────────────────────────────

/// No-op: ViCell surfaces don't have a title bar.
#[no_mangle]
pub extern "C" fn DG_SetWindowTitle(_title: *const u8) {}

// ─── Key translation ─────────────────────────────────────────────────────────

// DOOM key constants (from doomgeneric.h / doomkeys.h).
// dead_code: STRAFE/FIRE/PAUSE become active once Modifier key mapping is wired.
const DOOM_KEY_RIGHTARROW: u8 = 0xae;
const DOOM_KEY_LEFTARROW: u8 = 0xac;
const DOOM_KEY_UPARROW: u8 = 0xad;
const DOOM_KEY_DOWNARROW: u8 = 0xaf;
#[allow(dead_code)]
const DOOM_KEY_STRAFE_L: u8 = 0xa0;
#[allow(dead_code)]
const DOOM_KEY_STRAFE_R: u8 = 0xa1;
const DOOM_KEY_USE: u8 = 0xa2; // Space / Enter
#[allow(dead_code)]
const DOOM_KEY_FIRE: u8 = 0xa3; // Ctrl
const DOOM_KEY_ESCAPE: u8 = 27;
const DOOM_KEY_ENTER: u8 = 13;
const DOOM_KEY_TAB: u8 = 9;
const DOOM_KEY_F1: u8 = 0x80 + 0x3b;
const DOOM_KEY_F2: u8 = 0x80 + 0x3c;
const DOOM_KEY_F3: u8 = 0x80 + 0x3d;
const DOOM_KEY_F4: u8 = 0x80 + 0x3e;
const DOOM_KEY_F5: u8 = 0x80 + 0x3f;
const DOOM_KEY_F6: u8 = 0x80 + 0x40;
const DOOM_KEY_F7: u8 = 0x80 + 0x41;
const DOOM_KEY_F8: u8 = 0x80 + 0x42;
const DOOM_KEY_F9: u8 = 0x80 + 0x43;
const DOOM_KEY_F10: u8 = 0x80 + 0x44;
const DOOM_KEY_F11: u8 = 0x80 + 0x57;
const DOOM_KEY_F12: u8 = 0x80 + 0x58;
#[allow(dead_code)]
const DOOM_KEY_PAUSE: u8 = 0xff;

fn keysym_to_doom(sym: KeySym, character: u32) -> u8 {
    match sym {
        KeySym::Escape => DOOM_KEY_ESCAPE,
        KeySym::Return => DOOM_KEY_USE,
        KeySym::Tab => DOOM_KEY_TAB,
        KeySym::Up => DOOM_KEY_UPARROW,
        KeySym::Down => DOOM_KEY_DOWNARROW,
        KeySym::Left => DOOM_KEY_LEFTARROW,
        KeySym::Right => DOOM_KEY_RIGHTARROW,
        KeySym::F1 => DOOM_KEY_F1,
        KeySym::F2 => DOOM_KEY_F2,
        KeySym::F3 => DOOM_KEY_F3,
        KeySym::F4 => DOOM_KEY_F4,
        KeySym::F5 => DOOM_KEY_F5,
        KeySym::F6 => DOOM_KEY_F6,
        KeySym::F7 => DOOM_KEY_F7,
        KeySym::F8 => DOOM_KEY_F8,
        KeySym::F9 => DOOM_KEY_F9,
        KeySym::F10 => DOOM_KEY_F10,
        KeySym::F11 => DOOM_KEY_F11,
        KeySym::F12 => DOOM_KEY_F12,
        KeySym::Printable => {
            // Map printable to ASCII doom key.
            // DOOM checks lowercase keys; space=use, ctrl=fire(mapped via Modifiers),
            // alt=strafe (not yet handled — use as STRAFE_L for now).
            match character as u8 {
                b' ' => DOOM_KEY_USE,
                b'\r' | b'\n' => DOOM_KEY_ENTER,
                b'\t' => DOOM_KEY_TAB,
                c @ b'a'..=b'z' => c,
                c @ b'A'..=b'Z' => c + 32, // DOOM expects lowercase
                c @ b'0'..=b'9' => c,
                c => c,
            }
        }
        KeySym::Unknown
        | KeySym::Backspace
        | KeySym::Delete
        | KeySym::Insert
        | KeySym::Home
        | KeySym::End
        | KeySym::PageUp
        | KeySym::PageDown => 0,
    }
}
