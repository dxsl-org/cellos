// SPDX-License-Identifier: MIT
//! Grid View renderer for Files application (visual icon/thumbnail card browsing).

use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use viui::canvas::Color;

use crate::draw::{draw_label, fill_rect, stroke_rect};
use crate::model::FileItem;

pub const CARD_W: u32 = 100;
pub const CARD_H: u32 = 110;
pub const CARD_GAP: u32 = 12;

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

    let cols = ((content_w.saturating_sub(CARD_GAP)) / (CARD_W + CARD_GAP)).max(1) as usize;
    let pad_x = content_x + 12;
    let pad_y = content_y + 12;

    for (i, item) in items.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let cx = pad_x + col as i32 * (CARD_W + CARD_GAP) as i32;
        let cy = pad_y + row as i32 * (CARD_H + CARD_GAP) as i32;

        if cy + CARD_H as i32 > content_y + content_h as i32 {
            break;
        }

        let is_selected = i == selected_idx;

        if is_selected {
            fill_rect(surf, cx, cy, CARD_W, CARD_H, theme_accent.with_alpha(40));
            stroke_rect(surf, cx, cy, CARD_W, CARD_H, theme_accent);
        } else {
            fill_rect(surf, cx, cy, CARD_W, CARD_H, Color::rgb(250, 250, 252));
            stroke_rect(surf, cx, cy, CARD_W, CARD_H, theme_border);
        }

        // Preview box / icon
        let icon_box_y = cy + 10;
        let icon_box_h = 50;
        fill_rect(surf, cx + 15, icon_box_y, CARD_W - 30, icon_box_h, if is_selected { theme_accent.with_alpha(30) } else { Color::rgb(240, 242, 248) });
        draw_label(surf, fonts, cx + 20, icon_box_y + 12, CARD_W - 40, 24, item.icon, FontFace::UiSemibold, 18.0, if item.is_dir { theme_accent } else { theme_fg });

        // Label
        let label_y = cy + 68;
        draw_label(surf, fonts, cx + 6, label_y, CARD_W - 12, 18, &item.name, FontFace::UiRegular, 13.0, theme_fg);

        // Size subtext
        let size_str = item.formatted_size();
        draw_label(surf, fonts, cx + 6, label_y + 18, CARD_W - 12, 16, &size_str, FontFace::UiRegular, 11.0, Color::rgb(130, 135, 150));
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
    if x < content_x || x >= content_x + content_w as i32 || y < content_y {
        return None;
    }
    let cols = ((content_w.saturating_sub(CARD_GAP)) / (CARD_W + CARD_GAP)).max(1) as usize;
    let pad_x = content_x + 12;
    let pad_y = content_y + 12;

    let rel_x = x - pad_x;
    let rel_y = y - pad_y;
    if rel_x < 0 || rel_y < 0 {
        return None;
    }

    let col = (rel_x as u32 / (CARD_W + CARD_GAP)) as usize;
    let row = (rel_y as u32 / (CARD_H + CARD_GAP)) as usize;
    if col >= cols {
        return None;
    }

    let idx = row * cols + col;
    if idx < items_len {
        Some(idx)
    } else {
        None
    }
}
