// SPDX-License-Identifier: MIT
//! Places sidebar renderer for Files application.

use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use viui::canvas::Color;

use crate::draw::{draw_label, fill_rect};
use crate::model::PlacesItem;

pub const PLACES_WIDTH: u32 = 160;

pub fn render(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    places: &[PlacesItem],
    active_path: &str,
    view_h: u32,
    theme_bg: Color,
    theme_fg: Color,
    theme_accent: Color,
    theme_border: Color,
) {
    let sw = PLACES_WIDTH;
    let sh = view_h;

    // Sidebar background and right border
    fill_rect(surf, 0, 44, sw, sh, theme_bg);
    fill_rect(surf, sw as i32 - 1, 44, 1, sh, theme_border);

    // Section header: PLACES
    draw_label(surf, fonts, 14, 52, sw - 28, 20, "PLACES", FontFace::UiSemibold, 12.0, Color::rgb(130, 135, 150));

    let row_h = 28;
    let start_y = 76;

    for (i, item) in places.iter().enumerate() {
        let row_y = start_y + i as i32 * row_h;
        let is_active = active_path == item.path || (item.path != "/" && active_path.starts_with(item.path));

        if is_active {
            fill_rect(surf, 6, row_y, sw - 12, row_h as u32, theme_accent.with_alpha(60));
            fill_rect(surf, 6, row_y, 3, row_h as u32, theme_accent);
        }

        draw_label(surf, fonts, 14, row_y + 4, 24, 20, item.icon, FontFace::UiSemibold, 13.0, if is_active { theme_accent } else { theme_fg });

        // Label
        draw_label(surf, fonts, 42, row_y + 4, sw - 54, 20, item.name, FontFace::UiRegular, 14.0, theme_fg);
    }
}

pub fn handle_click(places: &[PlacesItem], x: i32, y: i32) -> Option<&'static str> {
    if x < 6 || x >= PLACES_WIDTH as i32 - 6 {
        return None;
    }
    let start_y = 76;
    let row_h = 28;
    if y < start_y {
        return None;
    }
    let idx = ((y - start_y) / row_h) as usize;
    places.get(idx).map(|item| item.path)
}
