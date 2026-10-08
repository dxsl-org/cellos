// SPDX-License-Identifier: MIT
//! Drawing primitives for CellOS Desktop.

use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};

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

#[allow(clippy::too_many_arguments)]
pub fn draw_text(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    x: f32,
    y: f32,
    text: &str,
    face: FontFace,
    px: f32,
    color: Color,
    clip: (i32, i32, i32, i32),
) {
    let width = surf.width();
    let height = surf.height();
    let stride = surf.stride();
    fonts.draw_text(
        surf.pixels_mut(),
        width,
        height,
        stride,
        x,
        y,
        text,
        face,
        px,
        color.as_bgra_bytes(),
        clip,
    );
}

/// Fit a single line into a rectangle; long labels are clipped at its edge.
#[allow(clippy::too_many_arguments)]
pub fn draw_label(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    text: &str,
    face: FontFace,
    px: f32,
    color: Color,
    centered: bool,
) {
    if w == 0 || h == 0 {
        return;
    }
    let text_x = if centered {
        x as f32 + (w as f32 - fonts.measure(face, text, px).min(w as f32)) / 2.0
    } else {
        x as f32
    };
    let text_y = y as f32 + (h as f32 - fonts.line_height(face, px)) / 2.0;
    draw_text(
        surf,
        fonts,
        text_x,
        text_y,
        text,
        face,
        px,
        color,
        (x, y, x + w as i32, y + h as i32),
    );
}

#[allow(clippy::too_many_arguments)]
pub fn draw_button(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
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
    draw_label(
        surf,
        fonts,
        x + 6,
        y + 1,
        w.saturating_sub(12),
        h.saturating_sub(2),
        text,
        FontFace::UiSemibold,
        14.0,
        fg,
        true,
    );
}
