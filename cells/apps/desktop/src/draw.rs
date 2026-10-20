// SPDX-License-Identifier: MIT
//! Drawing primitives for CellOS Desktop, delegated to standard ViUI surface painting.

pub use viui::canvas::Color;
pub use viui::surface::{draw_button, draw_text, fill_rect, stroke_rect};

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

pub mod theme {
    use viui::canvas::Color;
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
    pub const TEXT_PRIMARY: Color = Color::rgb(240, 242, 248);
    pub const TEXT_MUTED: Color = Color::rgb(140, 145, 160);
}
