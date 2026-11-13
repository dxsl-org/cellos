//! In-kernel linear framebuffer text console.
//!
//! Renders 8x16 characters onto the linear framebuffer and provides text scrolling.
//! Hands over framebuffer to userspace compositor on request.

use super::fb_font::FONT_8X16;
use crate::sync::Spinlock;
use core::sync::atomic::{AtomicBool, Ordering};

pub const FONT_WIDTH: usize = 8;
pub const FONT_HEIGHT: usize = 16;
pub const DEFAULT_FG_COLOR: u32 = 0x00EE_EEEE; // Clean light gray/white
pub const DEFAULT_BG_COLOR: u32 = 0x0000_0000; // Black

static FB_ACTIVE: AtomicBool = AtomicBool::new(false);
static FB_HANDED_OVER: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
pub struct FbConsoleState {
    pub virt_base: usize,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub cols: usize,
    pub rows: usize,
    pub cursor_col: usize,
    pub cursor_row: usize,
    pub fg_color: u32,
    pub bg_color: u32,
}

impl FbConsoleState {
    pub const fn empty() -> Self {
        Self {
            virt_base: 0,
            width: 0,
            height: 0,
            pitch: 0,
            cols: 0,
            rows: 0,
            cursor_col: 0,
            cursor_row: 0,
            fg_color: DEFAULT_FG_COLOR,
            bg_color: DEFAULT_BG_COLOR,
        }
    }

    pub fn init(&mut self, virt_base: usize, width: usize, height: usize, pitch: usize) {
        self.virt_base = virt_base;
        self.width = width;
        self.height = height;
        self.pitch = pitch;
        self.cols = width / FONT_WIDTH;
        self.rows = height / FONT_HEIGHT;
        self.cursor_col = 0;
        self.cursor_row = 0;
        self.fg_color = DEFAULT_FG_COLOR;
        self.bg_color = DEFAULT_BG_COLOR;

        // Clear screen to background color
        if virt_base != 0 && pitch >= 4 {
            let total_dwords = (height * pitch) / 4;
            let ptr = virt_base as *mut u32;
            for i in 0..total_dwords {
                unsafe {
                    ptr.add(i).write_volatile(self.bg_color);
                }
            }
        }
    }

    pub fn newline(&mut self) {
        self.cursor_col = 0;
        self.cursor_row += 1;
        if self.cursor_row >= self.rows {
            self.scroll_up(1);
            self.cursor_row = self.rows.saturating_sub(1);
        }
    }

    pub fn scroll_up(&mut self, lines: usize) {
        if self.virt_base == 0 || lines == 0 || lines >= self.rows {
            return;
        }
        let scroll_pixels = lines * FONT_HEIGHT;
        let copy_bytes = (self.height.saturating_sub(scroll_pixels)) * self.pitch;
        let src = self.virt_base + scroll_pixels * self.pitch;
        let dst = self.virt_base;

        // Copy screen lines up
        unsafe {
            core::ptr::copy(src as *const u8, dst as *mut u8, copy_bytes);
        }

        // Blank out bottom lines
        let blank_dwords = (scroll_pixels * self.pitch) / 4;
        let bottom_ptr = (dst + copy_bytes) as *mut u32;
        for i in 0..blank_dwords {
            unsafe {
                bottom_ptr.add(i).write_volatile(self.bg_color);
            }
        }
    }

    pub fn draw_glyph(&mut self, glyph: &[u8; 16], col: usize, row: usize) {
        if self.virt_base == 0 || col >= self.cols || row >= self.rows {
            return;
        }
        let x_base = col * FONT_WIDTH;
        let y_base = row * FONT_HEIGHT;

        for (y, &bits) in glyph.iter().enumerate() {
            let row_offset = (y_base + y) * self.pitch + x_base * 4;
            let row_ptr = (self.virt_base + row_offset) as *mut u32;
            for x in 0..FONT_WIDTH {
                let color = if (bits >> (7 - x)) & 1 != 0 {
                    self.fg_color
                } else {
                    self.bg_color
                };
                unsafe {
                    row_ptr.add(x).write_volatile(color);
                }
            }
        }
    }

    pub fn write_char(&mut self, c: u8) {
        if self.virt_base == 0 {
            return;
        }
        match c {
            b'\r' => {
                self.cursor_col = 0;
            }
            b'\n' => {
                self.newline();
            }
            b'\t' => {
                self.cursor_col = (self.cursor_col + 8) & !7;
                if self.cursor_col >= self.cols {
                    self.newline();
                }
            }
            0x08 if self.cursor_col > 0 => {
                self.cursor_col -= 1;
            }
            32..=126 => {
                let glyph = &FONT_8X16[(c - 32) as usize];
                self.draw_glyph(glyph, self.cursor_col, self.cursor_row);
                self.cursor_col += 1;
                if self.cursor_col >= self.cols {
                    self.newline();
                }
            }
            _ => {}
        }
    }
}

static FB_STATE: Spinlock<FbConsoleState> =
    Spinlock::ranked(FbConsoleState::empty(), crate::sync::rank::FB_STATE);

/// Initialize the framebuffer console with the provided geometry and virtual address.
pub fn init(virt_base: usize, width: usize, height: usize, pitch: usize) {
    let mut state = FB_STATE.lock();
    state.init(virt_base, width, height, pitch);
    FB_ACTIVE.store(true, Ordering::Release);
}

/// Release framebuffer control to userspace compositor/display cell.
pub fn handover() {
    FB_HANDED_OVER.store(true, Ordering::Release);
    FB_ACTIVE.store(false, Ordering::Release);
}

/// Check if framebuffer console is active and not yet handed over.
pub fn is_active() -> bool {
    FB_ACTIVE.load(Ordering::Acquire) && !FB_HANDED_OVER.load(Ordering::Acquire)
}

/// Render one byte onto the framebuffer console if active.
pub fn putchar(c: u8) {
    if !is_active() {
        return;
    }
    let mut state = FB_STATE.lock();
    state.write_char(c);
}

/// Render a string onto the framebuffer console if active.
pub fn puts(s: &str) {
    if !is_active() {
        return;
    }
    let mut state = FB_STATE.lock();
    for b in s.bytes() {
        state.write_char(b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fb_console_renders_and_scrolls() {
        let mut buffer = [0u32; 64 * 32]; // 64x32 pixels = 8 cols x 2 rows
        let virt_base = buffer.as_mut_ptr() as usize;
        let mut state = FbConsoleState::empty();
        state.init(virt_base, 64, 32, 64 * 4);

        assert_eq!(state.cols, 8);
        assert_eq!(state.rows, 2);
        assert_eq!(state.cursor_col, 0);
        assert_eq!(state.cursor_row, 0);

        // Render 'A'
        state.write_char(b'A');
        assert_eq!(state.cursor_col, 1);
        assert_eq!(state.cursor_row, 0);

        // Verify glyph is written in buffer
        assert!(buffer.iter().any(|&p| p == DEFAULT_FG_COLOR));

        // Fill line
        for _ in 1..8 {
            state.write_char(b'B');
        }
        assert_eq!(state.cursor_col, 0);
        assert_eq!(state.cursor_row, 1);

        // Fill second line to trigger scroll
        for _ in 0..8 {
            state.write_char(b'C');
        }
        assert_eq!(state.cursor_col, 0);
        assert_eq!(state.cursor_row, 1); // Scrolled up
    }
}
