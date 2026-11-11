// SPDX-License-Identifier: MIT
//! Drawing primitives for CellOS Desktop, delegated to standard ViUI surface painting.

pub use viui::canvas::Color;
pub use viui::surface::{
    draw_button, draw_icon_centered, draw_text, fill_rect, fill_round_rect, stroke_round_rect,
    LineIcon, CONTROL_RADIUS, PANEL_RADIUS,
};
pub use viui::theme::ViTheme;

// reason: thin rendering wrapper mirroring `draw_label_aligned`, called from many sites.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn draw_label(
    surf: &mut ostd::display::ViSurface,
    fonts: &mut ostd::typography::TextFonts,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    text: &str,
    face: ostd::typography::FontFace,
    size: f32,
    color: Color,
    center: bool,
) {
    viui::surface::draw_label_aligned(surf, fonts, x, y, w, h, text, face, size, color, center);
}
