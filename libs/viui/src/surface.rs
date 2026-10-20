// SPDX-License-Identifier: MIT
//! First-class ViSurface painting helpers and reusable UI primitives for ViUI.

use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use crate::canvas::Color;
use crate::theme::ViTheme;
pub mod components;
pub use components::*;

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

/// Draw a text label on a ViSurface with clipping and FontFace (left-aligned by default).
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

/// Draw raw text at `(x, y)` with clipping.
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
/// Draw a text label on a ViSurface with optional horizontal centering.
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
    draw_label_aligned(surf, fonts, x, y, w, h, text, FontFace::UiSemibold, 13.0, fg, true);
}

/// Draw badge / chip (e.g. `[HTTPS]`, format badge `[PDF]`, or count tag).
pub fn draw_badge(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    text: &str,
    bg: Color,
    fg: Color,
) {
    fill_rect(surf, x, y, w, h, bg);
    draw_label_aligned(surf, fonts, x, y, w, h, text, FontFace::UiSemibold, 11.0, fg, true);
}

// ─── Theme-aware High-Level Surface Helpers ─────────────────────────────────

/// Draw a themed panel/card container with background and border derived from `theme`.
pub fn draw_themed_panel(
    surf: &mut ViSurface,
    theme: &dyn ViTheme,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
) {
    fill_rect(surf, x, y, w, h, theme.surface());
    stroke_rect(surf, x, y, w, h, theme.border());
}

/// Draw a themed button using theme tokens (Normal / Hovered / Primary).
pub fn draw_themed_button(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    theme: &dyn ViTheme,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    text: &str,
    is_hovered: bool,
    is_primary: bool,
) {
    let (bg, border, fg) = if is_primary {
        (theme.accent(), theme.accent(), Color::WHITE)
    } else if is_hovered {
        (theme.button_hovered(), theme.border(), theme.text_primary())
    } else {
        (theme.button_normal(), theme.border(), theme.text_primary())
    };

    draw_button(surf, fonts, x, y, w, h, text, bg, border, fg);
}
