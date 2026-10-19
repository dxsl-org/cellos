// SPDX-License-Identifier: MIT
//! List View renderer for Files application (default tabular browsing).

use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use viui::canvas::Color;

use crate::draw::{draw_label, fill_rect};
use crate::model::FileItem;

pub const ROW_HEIGHT: u32 = 32;

pub fn render(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    items: &[FileItem],
    selected_idx: usize,
    content_x: i32,
    content_y: i32,
    content_w: u32,
    content_h: u32,
    theme_bg: Color,
    theme_fg: Color,
    theme_accent: Color,
    theme_border: Color,
) {
    // Background fill
    fill_rect(surf, content_x, content_y, content_w, content_h, theme_bg);

    // Header row
    let header_h = 28;
    fill_rect(surf, content_x, content_y, content_w, header_h, Color::rgb(240, 242, 246));
    fill_rect(surf, content_x, content_y + header_h as i32 - 1, content_w, 1, theme_border);

    draw_label(surf, fonts, content_x + 36, content_y + 4, content_w.saturating_sub(180), 20, "Name", FontFace::UiSemibold, 13.0, Color::rgb(110, 115, 130));
    draw_label(surf, fonts, content_x + content_w as i32 - 140, content_y + 4, 80, 20, "Size", FontFace::UiSemibold, 13.0, Color::rgb(110, 115, 130));
    draw_label(surf, fonts, content_x + content_w as i32 - 50, content_y + 4, 40, 20, "Kind", FontFace::UiSemibold, 13.0, Color::rgb(110, 115, 130));

    let list_y = content_y + header_h as i32;
    let max_rows = (content_h.saturating_sub(header_h) / ROW_HEIGHT) as usize;

    for (i, item) in items.iter().take(max_rows).enumerate() {
        let row_y = list_y + i as i32 * ROW_HEIGHT as i32;
        let is_selected = i == selected_idx;

        if is_selected {
            fill_rect(surf, content_x, row_y, content_w, ROW_HEIGHT, theme_accent.with_alpha(50));
            fill_rect(surf, content_x, row_y, 3, ROW_HEIGHT, theme_accent);
        }

        // Icon
        draw_label(surf, fonts, content_x + 8, row_y + 6, 24, 20, item.icon, FontFace::UiSemibold, 14.0, if item.is_dir { theme_accent } else { theme_fg });

        // Filename
        draw_label(surf, fonts, content_x + 36, row_y + 6, content_w.saturating_sub(180), 20, &item.name, FontFace::UiRegular, 14.0, theme_fg);

        // Size
        let size_str = item.formatted_size();
        draw_label(surf, fonts, content_x + content_w as i32 - 140, row_y + 6, 80, 20, &size_str, FontFace::UiRegular, 13.0, Color::rgb(110, 115, 130));

        // Kind
        let kind_str = if item.is_dir { "Folder" } else { "File" };
        draw_label(surf, fonts, content_x + content_w as i32 - 50, row_y + 6, 40, 20, kind_str, FontFace::UiRegular, 13.0, Color::rgb(110, 115, 130));

        // Row divider
        fill_rect(surf, content_x, row_y + ROW_HEIGHT as i32 - 1, content_w, 1, Color::rgb(240, 240, 244));
    }
}

pub fn handle_click(
    items_len: usize,
    content_x: i32,
    content_y: i32,
    content_w: u32,
    x: i32,
    y: i32,
) -> Option<usize> {
    if x < content_x || x >= content_x + content_w as i32 {
        return None;
    }
    let header_h = 28;
    let list_y = content_y + header_h;
    if y < list_y {
        return None;
    }
    let idx = ((y - list_y) / ROW_HEIGHT as i32) as usize;
    if idx < items_len {
        Some(idx)
    } else {
        None
    }
}
