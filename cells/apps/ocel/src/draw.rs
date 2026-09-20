// SPDX-License-Identifier: MIT
//! Drawing primitives for Ocel Document Viewer.

use crate::font::get_glyph;
use ostd::display::ViSurface;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

    pub const BG_DARK: Color = Color::rgb(30, 30, 46); // Mocha Base
    pub const BG_TOOLBAR: Color = Color::rgb(24, 24, 37); // Mantle
    pub const BG_INPUT: Color = Color::rgb(49, 50, 68); // Surface0
    pub const TEXT_PRIMARY: Color = Color::rgb(205, 214, 244);
    pub const TEXT_MUTED: Color = Color::rgb(166, 173, 200);
    pub const ACCENT_BLUE: Color = Color::rgb(137, 180, 250);
    pub const ACCENT_CYAN: Color = Color::rgb(148, 226, 213);
    pub const BORDER: Color = Color::rgb(69, 71, 90);
}

pub fn clear(surf: &mut ViSurface, color: Color) {
    let w = surf.width();
    let h = surf.height();
    fill_rect(surf, 0, 0, w, h, color);
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

pub fn draw_char(surf: &mut ViSurface, x: i32, y: i32, c: char, color: Color, scale: u32) {
    let sw = surf.width() as i32;
    let sh = surf.height() as i32;
    let stride = surf.stride();
    let pixels = surf.pixels_mut();
    let [b, g, r, a] = color.as_bgra_bytes();

    let glyph = get_glyph(c);
    let scale_i = scale as i32;
    for row in 0..8i32 {
        let mask = glyph[row as usize];
        for col in 0..8i32 {
            if mask & (0x80u8 >> col as u32) == 0 {
                continue;
            }
            for sy in 0..scale_i {
                let py = y + row * scale_i + sy;
                if py < 0 || py >= sh {
                    continue;
                }
                let row_start = py as usize * stride;
                for sx in 0..scale_i {
                    let px = x + col * scale_i + sx;
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
    let mut cx = x;
    for c in text.chars() {
        if c == '\n' {
            continue;
        }
        draw_char(surf, cx, y, c, color, scale);
        cx += 8 * scale as i32;
    }
}

pub fn draw_image(
    surf: &mut ViSurface,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    pixels: &[u8],
    src_stride: usize,
) {
    let sw = surf.width() as i32;
    let sh = surf.height() as i32;
    let dst_stride = surf.stride();
    let dst_pixels = surf.pixels_mut();

    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w as i32).min(sw);
    let y1 = (y + h as i32).min(sh);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    for py in y0..y1 {
        let src_y = (py - y) as usize;
        let src_row = src_y * src_stride;
        let dst_row = py as usize * dst_stride;

        for px in x0..x1 {
            let src_x = (px - x) as usize;
            let src_offset = src_row + src_x * 4;
            let dst_offset = dst_row + px as usize * 4;

            if src_offset + 3 < pixels.len() && dst_offset + 3 < dst_pixels.len() {
                let sa = pixels[src_offset + 3] as u32;
                if sa == 255 {
                    dst_pixels[dst_offset..dst_offset + 4]
                        .copy_from_slice(&pixels[src_offset..src_offset + 4]);
                } else if sa > 0 {
                    let inv = 255 - sa;
                    let sb = pixels[src_offset] as u32;
                    let sg = pixels[src_offset + 1] as u32;
                    let sr = pixels[src_offset + 2] as u32;

                    let db = dst_pixels[dst_offset] as u32;
                    let dg = dst_pixels[dst_offset + 1] as u32;
                    let dr = dst_pixels[dst_offset + 2] as u32;

                    dst_pixels[dst_offset] = ((sb * sa + db * inv) / 255) as u8;
                    dst_pixels[dst_offset + 1] = ((sg * sa + dg * inv) / 255) as u8;
                    dst_pixels[dst_offset + 2] = ((sr * sa + dr * inv) / 255) as u8;
                    dst_pixels[dst_offset + 3] = 255;
                }
            }
        }
    }
}
