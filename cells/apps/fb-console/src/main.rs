//! fb-console — mirrors kernel user-log output to the HDMI display.
//!
//! Creates a full-screen background surface in the compositor and renders
//! incoming log bytes as white-on-black 8×8 bitmap text. Wraps at the right
//! edge; scrolls one line up when the bottom row is reached. No UART I/O.
//!
//! Requires `ReadLog` capability (allowlist bit 54).

#![no_std]
#![no_main]

mod font;

extern crate ostd;

use api::display::PixelFormat;
use ostd::display::{wait_for_compositor, ViSurface};
use ostd::syscall::{sys_exit, sys_read_log, sys_yield};

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![Log, GpuGetResolution, GrantRegister, GrantShare, GrantSlice,
                       GrantUnregister, Send, Recv, LookupService, ReadLog];

// Foreground/background colours (BGRA).
const FG: [u8; 4] = [0xCC, 0xCC, 0xCC, 0xFF]; // light grey
const BG: [u8; 4] = [0x00, 0x00, 0x00, 0xFF]; // black

#[no_mangle]
pub fn main() {
    let (width, height) = ostd::syscall::sys_get_resolution();
    let width  = width  as usize;
    let height = height as usize;

    let comp = wait_for_compositor();
    let mut surf = match ViSurface::create(comp, width as u32, height as u32, PixelFormat::Bgra8888) {
        Ok(s)  => s,
        Err(_) => { sys_exit(1); }
    };

    // Clear to background.
    let px = surf.pixels_mut();
    for chunk in px.chunks_exact_mut(4) {
        chunk.copy_from_slice(&BG);
    }
    surf.damage_all();

    let cols = width  / font::WIDTH;
    let rows = height / font::HEIGHT;

    let mut col: usize = 0;
    let mut row: usize = 0;
    let mut buf = [0u8; 256];

    loop {
        let n = sys_read_log(&mut buf);
        if n == 0 {
            sys_yield();
            continue;
        }

        let mut dirty = false;
        for &b in &buf[..n] {
            match b {
                b'\n' => {
                    col = 0;
                    row += 1;
                    if row >= rows {
                        scroll_up(&mut surf, width, height, rows);
                        row = rows - 1;
                    }
                    dirty = true;
                }
                b'\r' => {
                    col = 0;
                }
                _ => {
                    draw_char(&mut surf, b, col, row, width);
                    col += 1;
                    dirty = true;
                    if col >= cols {
                        col = 0;
                        row += 1;
                        if row >= rows {
                            scroll_up(&mut surf, width, height, rows);
                            row = rows - 1;
                        }
                    }
                }
            }
        }
        if dirty {
            surf.damage_all();
        }
    }
}

/// Draw one ASCII character at grid cell (col, row).
fn draw_char(surf: &mut ViSurface, c: u8, col: usize, row: usize, _width: usize) {
    let glyph = font::glyph(c);
    let stride = surf.stride(); // bytes per row = width*4
    let px = surf.pixels_mut();
    let base_x = col * font::WIDTH;
    let base_y = row * font::HEIGHT;

    for (gy, &row_bits) in glyph.iter().enumerate() {
        let y = base_y + gy;
        for gx in 0..font::WIDTH {
            let x = base_x + gx;
            let off = y * stride + x * 4;
            if off + 4 > px.len() { continue; }
            let lit = (row_bits >> (7 - gx)) & 1 != 0;
            px[off..off + 4].copy_from_slice(if lit { &FG } else { &BG });
        }
    }
}

/// Scroll the framebuffer up by one text row.
fn scroll_up(surf: &mut ViSurface, width: usize, height: usize, rows: usize) {
    let stride  = width * 4;
    let row_h   = font::HEIGHT;
    let move_h  = (rows - 1) * row_h; // rows to keep
    let move_bytes = move_h * stride;
    let px = surf.pixels_mut();

    // Shift pixel data up by one text row.
    px.copy_within(row_h * stride..row_h * stride + move_bytes, 0);

    // Clear the last row.
    let clear_start = move_h * stride;
    let clear_end   = height * stride;
    for chunk in px[clear_start..clear_end].chunks_exact_mut(4) {
        chunk.copy_from_slice(&BG);
    }
}
