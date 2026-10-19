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
