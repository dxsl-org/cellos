// SPDX-License-Identifier: MIT
//! First-class ViSurface painting helpers and reusable UI primitives for ViUI.

use crate::canvas::Color;
use crate::theme::ViTheme;
use ostd::display::linear::{Bgra, Canvas, Radii, RoundRect};
use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
pub mod components;
pub mod icons;
pub use components::*;
pub use icons::{draw_icon, draw_icon_centered, paint_icon, LineIcon, ICON_SIZE};

pub use ostd::display::linear::{CONTROL_RADIUS, PANEL_RADIUS};

pub(crate) fn bgra(color: Color) -> Bgra {
    [color.b(), color.g(), color.r(), color.a()]
}

pub(crate) fn linear_canvas(surf: &mut ViSurface) -> Canvas<'_> {
    let (width, height, stride) = (surf.width(), surf.height(), surf.stride());
    Canvas::new(surf.pixels_mut(), width, height, stride)
}

/// Fill a rectangle directly on a ViSurface buffer using its native stride and bounds.
pub fn fill_rect(surf: &mut ViSurface, x: i32, y: i32, w: u32, h: u32, color: Color) {
    linear_canvas(surf).fill_rect(x, y, w, h, bgra(color));
}

/// Draw a 1-pixel rectangle outline on a ViSurface.
pub fn stroke_rect(surf: &mut ViSurface, x: i32, y: i32, w: u32, h: u32, color: Color) {
    linear_canvas(surf).stroke_rect(x, y, w, h, bgra(color));
}

/// Fill a rectangle with rounded corners of `radius` pixels.
///
/// Corner pixels outside the curve are left untouched, so whatever was painted
/// beneath shows through without reading it back.
// reason: public drawing primitive mirroring fill_rect plus the radius.
#[allow(clippy::too_many_arguments)]
pub fn fill_round_rect(
    surf: &mut ViSurface,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    radius: u32,
    color: Color,
) {
    linear_canvas(surf).fill_round(&RoundRect::new(x, y, w, h, Radii::all(radius)), bgra(color));
}

/// Draw the 1-pixel outline of [`fill_round_rect`] with the same geometry.
// reason: public drawing primitive mirroring stroke_rect plus the radius.
#[allow(clippy::too_many_arguments)]
pub fn stroke_round_rect(
    surf: &mut ViSurface,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    radius: u32,
    color: Color,
) {
    linear_canvas(surf).stroke_round(&RoundRect::new(x, y, w, h, Radii::all(radius)), bgra(color));
}

/// Horizontal inset, in pixels, between a left-aligned label's rect and its
/// first glyph; the text is clipped to the rect, so a rect that must show a
/// label whole is this inset plus the label's measured width.
pub const LABEL_TEXT_INSET: u32 = 8;

/// Draw a text label on a ViSurface with clipping and FontFace (left-aligned by default).
// reason: public drawing primitive used across app crates; its parameter list is its API.
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
    size: f32,
    color: Color,
) {
    draw_label_aligned(surf, fonts, x, y, w, h, text, face, size, color, false);
}

/// Draw raw text at `(x, y)` with clipping.
// reason: public drawing primitive used across app crates; its parameter list is its API.
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
        [color.b(), color.g(), color.r(), color.a()],
        clip,
    );
}
/// Draw a text label on a ViSurface with optional horizontal centering.
// reason: public drawing primitive used across app crates; its parameter list is its API.
#[allow(clippy::too_many_arguments)]
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
        x as f32 + LABEL_TEXT_INSET as f32
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

/// Draw standard toolkit button: rounded fill, 1px rounded border, centered label.
// reason: public drawing primitive used across app crates; its parameter list is its API.
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
    fill_round_rect(surf, x, y, w, h, CONTROL_RADIUS, bg);
    stroke_round_rect(surf, x, y, w, h, CONTROL_RADIUS, border);
    draw_label_aligned(
        surf,
        fonts,
        x,
        y,
        w,
        h,
        text,
        FontFace::UiSemibold,
        13.0,
        fg,
        true,
    );
}

/// Draw badge / chip (e.g. `[HTTPS]`, format badge `[PDF]`, or count tag).
// reason: public drawing primitive of the viui API, kept consistent with draw_button.
#[allow(clippy::too_many_arguments)]
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
    fill_round_rect(surf, x, y, w, h, CONTROL_RADIUS, bg);
    draw_label_aligned(
        surf,
        fonts,
        x,
        y,
        w,
        h,
        text,
        FontFace::UiSemibold,
        11.0,
        fg,
        true,
    );
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
    fill_round_rect(surf, x, y, w, h, PANEL_RADIUS, theme.surface());
    stroke_round_rect(surf, x, y, w, h, PANEL_RADIUS, theme.border());
}

/// Draw a themed button using theme tokens (Normal / Hovered / Primary).
// reason: public drawing primitive of the viui API, kept consistent with draw_button.
#[allow(clippy::too_many_arguments)]
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
