// SPDX-License-Identifier: MIT
//! Miller Columns View renderer for Files application (hierarchical cascading column browsing).

use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use viui::canvas::Color;

use crate::draw::{draw_label, fill_rect};
use crate::model::FileItem;

pub const COLUMN_W: u32 = 180;
pub const ROW_H: u32 = 28;

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
    fill_rect(surf, content_x, content_y, content_w, content_h, theme_bg);

    // Column 0: File/Folder listing
    let col0_x = content_x;
    let col0_w = COLUMN_W;
    fill_rect(surf, col0_x + col0_w as i32 - 1, content_y, 1, content_h, theme_border);

    let max_rows = (content_h / ROW_H) as usize;
    for (i, item) in items.iter().take(max_rows).enumerate() {
        let row_y = content_y + i as i32 * ROW_H as i32;
        let is_selected = i == selected_idx;

        if is_selected {
            fill_rect(surf, col0_x, row_y, col0_w - 1, ROW_H, theme_accent.with_alpha(50));
            fill_rect(surf, col0_x, row_y, 3, ROW_H, theme_accent);
        }

        // Icon
        draw_label(surf, fonts, col0_x + 8, row_y + 4, 20, 20, item.icon, FontFace::UiSemibold, 13.0, if item.is_dir { theme_accent } else { theme_fg });

        // Name
        draw_label(surf, fonts, col0_x + 30, row_y + 4, col0_w - 50, 20, &item.name, FontFace::UiRegular, 13.0, theme_fg);

        // Subdirectory arrow
        if item.is_dir {
            draw_label(surf, fonts, col0_x + col0_w as i32 - 18, row_y + 4, 12, 20, ">", FontFace::UiRegular, 12.0, Color::rgb(140, 145, 160));
        }
    }

    // Column 1: Preview / Details panel
    let col1_x = col0_x + col0_w as i32;
    let col1_w = content_w.saturating_sub(col0_w);
    if let Some(selected_item) = items.get(selected_idx) {
        let pad_x = col1_x + 16;
        let mut text_y = content_y + 20;

        // Large icon box
        fill_rect(surf, pad_x, text_y, 64, 64, Color::rgb(240, 242, 248));
        draw_label(surf, fonts, pad_x + 16, text_y + 18, 32, 28, selected_item.icon, FontFace::UiSemibold, 24.0, if selected_item.is_dir { theme_accent } else { theme_fg });

        text_y += 76;
        draw_label(surf, fonts, pad_x, text_y, col1_w.saturating_sub(32), 22, &selected_item.name, FontFace::UiSemibold, 16.0, theme_fg);

        text_y += 28;
        let kind_info = if selected_item.is_dir { "Folder" } else { "Document" };
        draw_label(surf, fonts, pad_x, text_y, col1_w.saturating_sub(32), 20, &alloc::format!("Kind: {}", kind_info), FontFace::UiRegular, 13.0, Color::rgb(120, 125, 140));

        text_y += 24;
        let size_info = selected_item.formatted_size();
        draw_label(surf, fonts, pad_x, text_y, col1_w.saturating_sub(32), 20, &alloc::format!("Size: {}", size_info), FontFace::UiRegular, 13.0, Color::rgb(120, 125, 140));

        text_y += 24;
        draw_label(surf, fonts, pad_x, text_y, col1_w.saturating_sub(32), 20, &alloc::format!("Location: {}", selected_item.path), FontFace::UiRegular, 13.0, Color::rgb(120, 125, 140));

        text_y += 36;
        draw_label(surf, fonts, pad_x, text_y, col1_w.saturating_sub(32), 20, "Space Quick Look · Enter Open", FontFace::UiSemibold, 13.0, theme_accent);
    }
}

pub fn handle_click(
    items_len: usize,
    content_x: i32,
    content_y: i32,
    x: i32,
    y: i32,
) -> Option<usize> {
    if x < content_x || x >= content_x + COLUMN_W as i32 || y < content_y {
        return None;
    }
    let idx = ((y - content_y) / ROW_H as i32) as usize;
    if idx < items_len {
        Some(idx)
    } else {
        None
    }
}
