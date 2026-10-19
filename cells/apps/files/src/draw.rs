// SPDX-License-Identifier: MIT
//! Shared rendering primitives for Files application.

use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use viui::canvas::Color;

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

pub fn stroke_rect(surf: &mut ViSurface, x: i32, y: i32, w: u32, h: u32, color: Color) {
    fill_rect(surf, x, y, w, 1, color);
    fill_rect(surf, x, y + h as i32 - 1, w, 1, color);
    fill_rect(surf, x, y, 1, h, color);
    fill_rect(surf, x + w as i32 - 1, y, 1, h, color);
}

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
        [color.b(), color.g(), color.r(), color.a()],
        clip,
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
    draw_label(surf, fonts, x + 4, y, w.saturating_sub(8), h, text, FontFace::UiSemibold, 13.0, fg);
}
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
    if w == 0 || h == 0 {
        return;
    }
    let text_x = x as f32;
    let text_y = y as f32 + (h as f32 - fonts.line_height(face, size)) / 2.0;
    draw_text(
        surf,
        fonts,
        text_x,
        text_y,
        text,
        face,
        size,
        color,
        (x, y, x + w as i32, y + h as i32),
    );
}
