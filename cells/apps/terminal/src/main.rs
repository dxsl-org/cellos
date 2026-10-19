// SPDX-License-Identifier: MIT
//! CellOS Native GUI Terminal Cell (Monospace Grid, Single-Reader Stdio Lease, Shell Integration).

#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

extern crate alloc;

mod grid;
mod session;

use api::display::PixelFormat;
use ostd::display::ViSurface;
use ostd::input::{InputEvent, KeyState, KeySym, Modifiers};
use ostd::syscall::{sys_get_resolution, sys_yield};
use ostd::typography::{FontFace, TextFonts};
use viui::canvas::Color;

use crate::session::TerminalSession;

api::declare_manifest!(block_io = false, network = false, spawn = true);
api::declare_syscalls![
    Log,
    SpawnFromPath,
    GrantAlloc,
    GrantRegister,
    GrantShare,
    GrantSlice,
    GrantUnregister,
    Send,
    Recv,
    TryRecv,
    LookupService,
    GpuGetResolution,
    GetTime
];

ostd::cell_main!(cell_main);

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(4 * 1024 * 1024);

pub struct TerminalApp {
    pub session: TerminalSession,
    pub width: u32,
    pub height: u32,
    pub cell_w: f32,
    pub cell_h: f32,
}

impl TerminalApp {
    pub fn new(width: u32, height: u32) -> Self {
        let cell_w = 9.0;
        let cell_h = 18.0;
        let cols = (width as f32 / cell_w).max(40.0) as usize;
        let rows = (height as f32 / cell_h).max(12.0) as usize;

        Self {
            session: TerminalSession::new(1, cols, rows),
            width,
            height,
            cell_w,
            cell_h,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        let cols = (width as f32 / self.cell_w).max(40.0) as usize;
        let rows = (height as f32 / self.cell_h).max(12.0) as usize;
        self.session.grid.resize(cols, rows);
    }

    pub fn render(&self, surf: &mut ViSurface, fonts: &mut TextFonts) {
        let sw = self.width;
        let sh = self.height;
        let surf_w = surf.width();
        let surf_h = surf.height();
        let stride = surf.stride();
        // Dark terminal background
        fill_rect(surf, 0, 0, sw, sh, Color::rgb(18, 20, 26));

        let pad_x = 8;
        let pad_y = 6;
        let font_size = 14.0;
        let line_h = fonts.line_height(FontFace::UiRegular, font_size);
        let ascender = fonts.ascender(FontFace::UiRegular, font_size);

        for r in 0..self.session.grid.rows {
            let row_y = pad_y as f32 + r as f32 * self.cell_h;
            if row_y + self.cell_h > sh as f32 {
                break;
            }
            let _baseline = row_y + (self.cell_h - line_h) * 0.5 + ascender;

            for c in 0..self.session.grid.cols {
                let col_x = pad_x as f32 + c as f32 * self.cell_w;
                if col_x + self.cell_w > sw as f32 {
                    break;
                }

                if let Some(cell) = self.session.grid.cell_at(c, r) {
                    if cell.ch != ' ' {
                        let mut ch_buf = [0u8; 4];
                        let text = cell.ch.encode_utf8(&mut ch_buf);
                        let clip = (col_x as i32, row_y as i32, (col_x + self.cell_w) as i32 + 2, (row_y + self.cell_h) as i32);
                        fonts.draw_text(
                            surf.pixels_mut(),
                            surf_w,
                            surf_h,
                            stride,
                            col_x,
                            row_y,
                            &text,
                            FontFace::UiRegular,
                            font_size,
                            [cell.fg.b(), cell.fg.g(), cell.fg.r(), cell.fg.a()],
                            clip,
                        );
                    }
                }
            }
        }

        // Draw cursor block
        let cur_x = pad_x as f32 + self.session.grid.cursor_x as f32 * self.cell_w;
        let cur_y = pad_y as f32 + self.session.grid.cursor_y as f32 * self.cell_h;
        if cur_x + self.cell_w <= sw as f32 && cur_y + self.cell_h <= sh as f32 {
            fill_rect(surf, cur_x as i32, cur_y as i32 + 2, self.cell_w as u32, (self.cell_h - 4.0) as u32, Color::rgb(0, 200, 255).with_alpha(160));
        }
    }
}

fn cell_main() {
    #[cfg(target_os = "none")]
    init_custom_heap();

    ostd::io::println("[terminal] CellOS GUI Terminal initializing...");
    let comp_tid = ostd::display::wait_for_compositor();

    let (mut screen_w, mut screen_h) = sys_get_resolution();
    if screen_w == 0 || screen_h == 0 || screen_w > 4096 || screen_h > 4096 {
        screen_w = 1280;
        screen_h = 800;
    }

    let win_w = 720.min(screen_w);
    let win_h = 480.min(screen_h - 36);

    let mut surf = ViSurface::create(comp_tid, win_w, win_h, PixelFormat::Bgra8888)
        .expect("create terminal surface");
    let _ = surf.set_title("Terminal — CellOS Shell");
    surf.move_to(140, 100);

    let mut app = TerminalApp::new(win_w, win_h);
    let mut fonts = TextFonts::new();

    while !ostd::input::request_focus() {
        sys_yield();
    }

    app.render(&mut surf, &mut fonts);
    ostd::io::println("[terminal] Terminal surface ready. Entering event loop.");

    loop {
        let mut need_render = false;

        for ev in ostd::input::poll_events(16) {
            match ev {
                InputEvent::Key(ke) if ke.state == KeyState::Pressed => {
                    if ke.modifiers.contains(Modifiers::CTRL) && ke.char() == Some('c') {
                        app.session.send_interrupt();
                        need_render = true;
                    } else if ke.keysym == KeySym::Return {
                        app.session.feed_char('\n');
                        need_render = true;
                    } else if ke.keysym == KeySym::Backspace {
                        app.session.feed_char('\x08');
                        need_render = true;
                    } else if let Some(ch) = ke.char() {
                        if !ch.is_control() {
                            app.session.feed_char(ch);
                            need_render = true;
                        }
                    }
                }
                _ => {}
            }
        }

        if app.session.should_close {
            ostd::io::println("[terminal] Exiting terminal application.");
            break;
        }

        if need_render {
            app.render(&mut surf, &mut fonts);
        }

        sys_yield();
    }
}

fn fill_rect(surf: &mut ViSurface, x: i32, y: i32, w: u32, h: u32, color: Color) {
    let sw = surf.width() as i32;
    let sh = surf.height() as i32;
    let stride = surf.stride();
    let pixels = surf.pixels_mut();

    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = x.saturating_add(w as i32).min(sw);
    let y1 = y.saturating_add(h as i32).min(sh);

    for cy in y0..y1 {
        for cx in x0..x1 {
            let offset = (cy as usize * stride + cx as usize) * 4;
            if offset + 4 <= pixels.len() {
                pixels[offset] = color.b();
                pixels[offset + 1] = color.g();
                pixels[offset + 2] = color.r();
                pixels[offset + 3] = color.a();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_app_lifecycle_and_resize() {
        let mut app = TerminalApp::new(720, 480);
        assert!(app.session.has_reader_lease);
        assert!(!app.session.should_close);

        // Feed command
        for ch in "help\n".chars() {
            app.session.feed_char(ch);
        }
        assert!(app.session.grid.cursor_y > 0);

        // Resize
        app.resize(800, 600);
        assert!(app.session.grid.cols >= 40);
        assert!(app.session.grid.rows >= 12);
    }
}
