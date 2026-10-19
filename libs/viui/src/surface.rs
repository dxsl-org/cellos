// SPDX-License-Identifier: MIT
//! First-class ViSurface painting helpers and reusable UI primitives for ViUI.

use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use crate::canvas::Color;

/// Fill a rectangle directly on a ViSurface buffer using its native stride and bounds.
pub fn fill_rect(surf: &mut ViSurface, x: i32, y: i32, w: u32, h: u32, color: Color) {
    let sw = surf.width() as i32;
    let sh = surf.height() as i32;
    let stride = surf.stride();
    let pixels = surf.pixels_mut();

    let x0 = x.clamp(0, sw);
    let y0 = y.clamp(0, sh);
    let x1 = x.saturating_add(w as i32).clamp(0, sw);
    let y1 = y.saturating_add(h as i32).clamp(0, sh);

    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let bgra = [color.b(), color.g(), color.r(), color.a()];

    for cy in y0..y1 {
        let row_start = (cy as usize * stride) + (x0 as usize * 4);
        for cx in 0..(x1 - x0) as usize {
            let offset = row_start + (cx * 4);
            if offset + 4 <= pixels.len() {
                pixels[offset..offset + 4].copy_from_slice(&bgra);
            }
        }
    }
}

/// Draw a 1-pixel rectangle outline on a ViSurface.
pub fn stroke_rect(surf: &mut ViSurface, x: i32, y: i32, w: u32, h: u32, color: Color) {
    fill_rect(surf, x, y, w, 1, color);
    fill_rect(surf, x, y + h as i32 - 1, w, 1, color);
    fill_rect(surf, x, y, 1, h, color);
    fill_rect(surf, x + w as i32 - 1, y, 1, h, color);
}

/// Draw a text label on a ViSurface with clipping and FontFace.
pub fn draw_label(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    text: &str,
    face: FontFace,
    size: f32,
    color: Color,
) {
    draw_label_aligned(surf, fonts, x, y, w, h, text, face, size, color, false);
}

pub fn draw_label_centered(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    text: &str,
    face: FontFace,
    size: f32,
    color: Color,
) {
    draw_label_aligned(surf, fonts, x, y, w, h, text, face, size, color, true);
}

pub fn draw_label_aligned(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    text: &str,
    face: FontFace,
    size: f32,
    color: Color,
    center: bool,
) {
    let line_h = fonts.line_height(face, size);
    let text_y = y as f32 + (h as f32 - line_h) * 0.5;
    let text_x = if center {
        let text_w = fonts.measure(face, text, size);
        x as f32 + (w as f32 - text_w) * 0.5
    } else {
        x as f32 + 8.0
    };
    let clip = (x, y, x + w as i32, y + h as i32);
    let width = surf.width();
    let height = surf.height();
    let stride = surf.stride();
    fonts.draw_text(
        surf.pixels_mut(),
        width,
        height,
        stride,
        text_x,
        text_y,
        text,
        face,
        size,
        [color.b(), color.g(), color.r(), color.a()],
        clip,
    );
}

/// Draw standard toolkit button with background, border, and centered label.
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
    draw_label_centered(surf, fonts, x, y, w, h, text, FontFace::UiSemibold, 13.0, fg);
}
