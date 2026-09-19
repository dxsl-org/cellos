// SPDX-License-Identifier: MIT
//! Drawing primitives for CellOS Desktop.

use ostd::display::ViSurface;
use ostd::font::FONT8X8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub b: u8,
    pub g: u8,
    pub r: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    #[allow(dead_code)]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn as_bgra_bytes(self) -> [u8; 4] {
        [self.b, self.g, self.r, self.a]
    }
}

pub mod theme {
    use super::Color;
    pub const TASKBAR_BG: Color = Color::rgb(26, 28, 35);
    pub const TASKBAR_BORDER: Color = Color::rgb(50, 54, 66);
    pub const MODAL_BG: Color = Color::rgb(22, 24, 30);
    pub const MODAL_BORDER: Color = Color::rgb(65, 70, 85);
    pub const BTN_NORMAL: Color = Color::rgb(38, 41, 52);
    pub const BTN_HOVER: Color = Color::rgb(52, 57, 72);
    pub const BTN_ACTIVE: Color = Color::rgb(68, 75, 96);
    pub const ACCENT_CYAN: Color = Color::rgb(0, 200, 220);
    pub const ACCENT_BLUE: Color = Color::rgb(70, 130, 245);
    pub const ACCENT_GREEN: Color = Color::rgb(45, 195, 95);
    pub const ACCENT_RED: Color = Color::rgb(230, 70, 70);
    pub const TEXT_PRIMARY: Color = Color::rgb(240, 242, 248);
    pub const TEXT_MUTED: Color = Color::rgb(140, 145, 160);
}

pub fn fill_rect(surf: &mut ViSurface, x: i32, y: i32, w: u32, h: u32, color: Color) {
    let sw = surf.width() as i32;
    let sh = surf.height() as i32;
    let stride = surf.stride();
    let pixels = surf.pixels_mut();

    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w as i32).min(sw);
    let y1 = (y + h as i32).min(sh);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let [b, g, r, a] = color.as_bgra_bytes();
    for py in y0..y1 {
        let row_start = py as usize * stride;
        for px in x0..x1 {
            let offset = row_start + px as usize * 4;
            pixels[offset] = b;
            pixels[offset + 1] = g;
            pixels[offset + 2] = r;
            pixels[offset + 3] = a;
        }
    }
}

pub fn stroke_rect(surf: &mut ViSurface, x: i32, y: i32, w: u32, h: u32, border: Color) {
    if w == 0 || h == 0 {
        return;
    }
    fill_rect(surf, x, y, w, 1, border);
    fill_rect(surf, x, y + h as i32 - 1, w, 1, border);
    fill_rect(surf, x, y, 1, h, border);
    fill_rect(surf, x + w as i32 - 1, y, 1, h, border);
}

pub fn draw_char(surf: &mut ViSurface, x: i32, y: i32, c: u8, color: Color, scale: u32) {
    let sw = surf.width() as i32;
    let sh = surf.height() as i32;
    let stride = surf.stride();
    let pixels = surf.pixels_mut();
    let [b, g, r, a] = color.as_bgra_bytes();

    let idx = if (0x20..=0x7E).contains(&c) {
        (c - 0x20) as usize
    } else {
        0
    };

    let scale_i = scale as i32;
    for row in 0..8i32 {
        let mask = FONT8X8[idx][row as usize];
        for col in 0..8i32 {
            if mask & (0x80u8 >> col as u32) == 0 {
                continue;
            }
            for dy in 0..scale_i {
                let py = y + row * scale_i + dy;
                if py < 0 || py >= sh {
                    continue;
                }
                let row_start = py as usize * stride;
                for dx in 0..scale_i {
                    let px = x + col * scale_i + dx;
                    if px < 0 || px >= sw {
                        continue;
                    }
                    let offset = row_start + px as usize * 4;
                    pixels[offset] = b;
                    pixels[offset + 1] = g;
                    pixels[offset + 2] = r;
                    pixels[offset + 3] = a;
                }
            }
        }
    }
}

pub fn draw_str(surf: &mut ViSurface, x: i32, y: i32, text: &str, color: Color, scale: u32) {
    let mut cursor_x = x;
    let char_w = 8 * scale as i32;
    for &byte in text.as_bytes() {
        draw_char(surf, cursor_x, y, byte, color, scale);
        cursor_x += char_w;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_button(
    surf: &mut ViSurface,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    text: &str,
    bg: Color,
    border: Color,
    fg: Color,
) {
    fill_rect(surf, x, y, w, h, bg);
    stroke_rect(surf, x, y, w, h, border);
    let text_w = text.len() as i32 * 8;
    let text_x = x + (w as i32 - text_w) / 2;
    let text_y = y + (h as i32 - 8) / 2;
    draw_str(surf, text_x, text_y, text, fg, 1);
}
