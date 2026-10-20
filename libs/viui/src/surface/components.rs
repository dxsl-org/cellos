// SPDX-License-Identifier: MIT
//! High-level UI Components for ViSurface applications (Toolbar, SegmentedControl, BreadcrumbBar, PlacesSidebar, ListView, GridView, StatusBar).
//!
//! All components automatically read geometry, typography, and semantic colors from `ViTheme`.

extern crate alloc;
use alloc::string::String;

use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use crate::canvas::Color;
use crate::surface::{draw_label, draw_label_aligned, fill_rect, stroke_rect};
use crate::theme::ViTheme;

// ─── 1. Toolbar Component ────────────────────────────────────────────────────

pub struct Toolbar;

impl Toolbar {
    pub const DEFAULT_HEIGHT: u32 = 44;

    /// Render a standard top toolbar with app icon, title/path, and bottom border.
    pub fn render(
        surf: &mut ViSurface,
        fonts: &mut TextFonts,
        theme: &dyn ViTheme,
        width: u32,
        height: u32,
        icon: &str,
        title: &str,
    ) {
        // Toolbar background
        fill_rect(surf, 0, 0, width, height, theme.surface());
        // Bottom divider
        fill_rect(surf, 0, height as i32 - 1, width, 1, theme.border());

        // App Icon
        if !icon.is_empty() {
            draw_label(
                surf,
                fonts,
                12,
                (height as i32 - 24) / 2,
                32,
                24,
                icon,
                FontFace::UiSemibold,
                16.0,
                theme.accent(),
            );
        }

        // Title / Breadcrumb
        let text_x = if icon.is_empty() { 16 } else { 48 };
        let text_w = width.saturating_sub(text_x as u32 + 200);
        draw_label(
            surf,
            fonts,
            text_x,
            (height as i32 - 24) / 2,
            text_w,
            24,
            title,
            FontFace::UiSemibold,
            15.0,
            theme.text_primary(),
        );
    }
}

// ─── 2. SegmentedControl Component ───────────────────────────────────────────

pub struct SegmentedControl;

impl SegmentedControl {
    /// Render a connected segmented pill control (e.g. `[List] [Grid] [Cols]`).
    pub fn render(
        surf: &mut ViSurface,
        fonts: &mut TextFonts,
        theme: &dyn ViTheme,
        x: i32,
        y: i32,
        item_w: u32,
        h: u32,
        items: &[&str],
        selected_idx: usize,
    ) {
        for (i, label) in items.iter().enumerate() {
            let item_x = x + i as i32 * (item_w as i32 + 2);
            let is_selected = i == selected_idx;

            let (bg, border, fg) = if is_selected {
                (theme.accent(), theme.accent(), Color::WHITE)
            } else {
                (theme.button_normal(), theme.border(), theme.text_secondary())
            };

            fill_rect(surf, item_x, y, item_w, h, bg);
            stroke_rect(surf, item_x, y, item_w, h, border);
            draw_label_aligned(
                surf,
                fonts,
                item_x,
                y,
                item_w,
                h,
                label,
                FontFace::UiSemibold,
                13.0,
                fg,
                true,
            );
        }
    }

    /// Hit-test click event against SegmentedControl items. Returns `Some(index)` if hit.
    pub fn hit_test(
        click_x: i32,
        click_y: i32,
        x: i32,
        y: i32,
        item_w: u32,
        h: u32,
        count: usize,
    ) -> Option<usize> {
        if click_y < y || click_y >= y + h as i32 {
            return None;
        }
        for i in 0..count {
            let item_x = x + i as i32 * (item_w as i32 + 2);
            if click_x >= item_x && click_x < item_x + item_w as i32 {
                return Some(i);
            }
        }
        None
    }
}

// ─── 3. PlacesSidebar Component ──────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacesItemDef {
    pub name: String,
    pub path: String,
    pub icon: String,
}

pub struct PlacesSidebar;

impl PlacesSidebar {
    pub const DEFAULT_WIDTH: u32 = 160;
    pub const ROW_HEIGHT: u32 = 28;

    /// Render a standard navigation sidebar (Places / Bookmarks).
    pub fn render(
        surf: &mut ViSurface,
        fonts: &mut TextFonts,
        theme: &dyn ViTheme,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        section_title: &str,
        items: &[PlacesItemDef],
        active_path: &str,
    ) {
        // Sidebar background
        fill_rect(surf, x, y, width, height, theme.surface());
        // Right vertical divider
        fill_rect(surf, x + width as i32 - 1, y, 1, height, theme.border());

        // Section header
        draw_label(
            surf,
            fonts,
            x + 14,
            y + 8,
            width.saturating_sub(28),
            20,
            section_title,
            FontFace::UiSemibold,
            11.0,
            theme.text_secondary(),
        );

        let row_start_y = y + 32;
        for (i, item) in items.iter().enumerate() {
            let row_y = row_start_y + i as i32 * Self::ROW_HEIGHT as i32;
            if row_y + Self::ROW_HEIGHT as i32 > y + height as i32 {
                break;
            }

            let is_active = active_path == item.path || (item.path == "/" && active_path.is_empty());

            if is_active {
                // Highlight pill background
                fill_rect(
                    surf,
                    x + 6,
                    row_y,
                    width.saturating_sub(12),
                    Self::ROW_HEIGHT,
                    theme.list_selected_bg(),
                );
                // Active accent indicator (3px on left)
                fill_rect(surf, x + 6, row_y, 3, Self::ROW_HEIGHT, theme.accent());
            }

            // Icon
            let icon_col = if is_active { theme.accent() } else { theme.text_secondary() };
            draw_label(
                surf,
                fonts,
                x + 14,
                row_y + 4,
                24,
                20,
                &item.icon,
                FontFace::UiSemibold,
                13.0,
                icon_col,
            );

            // Name
            let name_col = if is_active { theme.text_primary() } else { theme.text_secondary() };
            draw_label(
                surf,
                fonts,
                x + 42,
                row_y + 4,
                width.saturating_sub(54),
                20,
                &item.name,
                FontFace::UiRegular,
                14.0,
                name_col,
            );
        }
    }

    /// Hit-test click event against PlacesSidebar items. Returns `Some(path)` if hit.
    pub fn hit_test<'a>(
        click_x: i32,
        click_y: i32,
        x: i32,
        y: i32,
        width: u32,
        items: &'a [PlacesItemDef],
    ) -> Option<&'a str> {
        if click_x < x || click_x >= x + width as i32 {
            return None;
        }
        let row_start_y = y + 32;
        if click_y < row_start_y {
            return None;
        }
        let row_idx = (click_y - row_start_y) / Self::ROW_HEIGHT as i32;
        if row_idx >= 0 && (row_idx as usize) < items.len() {
            Some(&items[row_idx as usize].path)
        } else {
            None
        }
    }
}

// ─── 4. ListView Component ───────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ListViewItemDef {
    pub icon: String,
    pub name: String,
    pub size_str: String,
    pub kind_str: String,
    pub is_dir: bool,
}

pub struct ListView;

impl ListView {
    pub const HEADER_HEIGHT: u32 = 28;
    pub const ROW_HEIGHT: u32 = 32;

    /// Render a multi-column data list with header and selectable rows.
    pub fn render(
        surf: &mut ViSurface,
        fonts: &mut TextFonts,
        theme: &dyn ViTheme,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        items: &[ListViewItemDef],
        selected_idx: Option<usize>,
    ) {
        // Background fill
        fill_rect(surf, x, y, width, height, theme.bg());

        // Header row
        fill_rect(surf, x, y, width, Self::HEADER_HEIGHT, theme.surface());
        fill_rect(surf, x, y + Self::HEADER_HEIGHT as i32 - 1, width, 1, theme.border());

        let col_name_w = width.saturating_sub(220);
        draw_label(surf, fonts, x + 36, y + 4, col_name_w, 20, "Name", FontFace::UiSemibold, 12.0, theme.text_secondary());
        draw_label(surf, fonts, x + width as i32 - 170, y + 4, 80, 20, "Size", FontFace::UiSemibold, 12.0, theme.text_secondary());
        draw_label(surf, fonts, x + width as i32 - 70, y + 4, 60, 20, "Kind", FontFace::UiSemibold, 12.0, theme.text_secondary());

        let rows_start_y = y + Self::HEADER_HEIGHT as i32;
        let max_visible = (height.saturating_sub(Self::HEADER_HEIGHT) / Self::ROW_HEIGHT) as usize;

        for (i, item) in items.iter().take(max_visible).enumerate() {
            let row_y = rows_start_y + i as i32 * Self::ROW_HEIGHT as i32;
            let is_selected = selected_idx == Some(i);

            if is_selected {
                fill_rect(surf, x, row_y, width, Self::ROW_HEIGHT, theme.list_selected_bg());
                fill_rect(surf, x, row_y, 3, Self::ROW_HEIGHT, theme.accent());
            }

            // Icon
            let icon_col = if item.is_dir { theme.accent() } else { theme.text_primary() };
            draw_label(surf, fonts, x + 8, row_y + 6, 24, 20, &item.icon, FontFace::UiSemibold, 14.0, icon_col);

            // Name
            let name_col = if is_selected { theme.text_primary() } else { theme.text_primary() };
            draw_label(surf, fonts, x + 36, row_y + 6, col_name_w, 20, &item.name, FontFace::UiRegular, 14.0, name_col);

            // Size
            draw_label(surf, fonts, x + width as i32 - 170, row_y + 6, 80, 20, &item.size_str, FontFace::UiRegular, 13.0, theme.text_secondary());

            // Kind
            draw_label(surf, fonts, x + width as i32 - 70, row_y + 6, 60, 20, &item.kind_str, FontFace::UiRegular, 13.0, theme.text_secondary());

            // Row divider
            fill_rect(surf, x, row_y + Self::ROW_HEIGHT as i32 - 1, width, 1, theme.divider());
        }
    }

    /// Hit-test click event against ListView items. Returns `Some(index)` if row clicked.
    pub fn hit_test(
        click_x: i32,
        click_y: i32,
        x: i32,
        y: i32,
        width: u32,
        count: usize,
    ) -> Option<usize> {
        if click_x < x || click_x >= x + width as i32 {
            return None;
        }
        let rows_start_y = y + Self::HEADER_HEIGHT as i32;
        if click_y < rows_start_y {
            return None;
        }
        let idx = (click_y - rows_start_y) / Self::ROW_HEIGHT as i32;
        if idx >= 0 && (idx as usize) < count {
            Some(idx as usize)
        } else {
            None
        }
    }
}

// ─── 5. GridView Component ───────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct GridItemDef {
    pub icon: String,
    pub name: String,
    pub is_dir: bool,
}

pub struct GridView;

impl GridView {
    pub const CARD_W: u32 = 104;
    pub const CARD_H: u32 = 96;
    pub const CARD_GAP: u32 = 12;

    /// Render items in a responsive grid of cards.
    pub fn render(
        surf: &mut ViSurface,
        fonts: &mut TextFonts,
        theme: &dyn ViTheme,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        items: &[GridItemDef],
        selected_idx: Option<usize>,
    ) {
        fill_rect(surf, x, y, width, height, theme.bg());

        let cols = ((width.saturating_sub(Self::CARD_GAP)) / (Self::CARD_W + Self::CARD_GAP)).max(1) as usize;
        let pad_x = x + 12;
        let pad_y = y + 12;

        for (i, item) in items.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let cx = pad_x + col as i32 * (Self::CARD_W + Self::CARD_GAP) as i32;
            let cy = pad_y + row as i32 * (Self::CARD_H + Self::CARD_GAP) as i32;

            if cy + Self::CARD_H as i32 > y + height as i32 {
                break;
            }

            let is_selected = selected_idx == Some(i);

            if is_selected {
                fill_rect(surf, cx, cy, Self::CARD_W, Self::CARD_H, theme.list_selected_bg());
                stroke_rect(surf, cx, cy, Self::CARD_W, Self::CARD_H, theme.accent());
            } else {
                fill_rect(surf, cx, cy, Self::CARD_W, Self::CARD_H, theme.surface());
                stroke_rect(surf, cx, cy, Self::CARD_W, Self::CARD_H, theme.border());
            }

            // Inner icon box
            let icon_box_y = cy + 8;
            let icon_box_h = 44;
            let icon_box_bg = if is_selected { theme.list_selected_bg() } else { theme.bg() };
            fill_rect(surf, cx + 12, icon_box_y, Self::CARD_W - 24, icon_box_h, icon_box_bg);

            let icon_fg = if item.is_dir { theme.accent() } else { theme.text_primary() };
            draw_label_aligned(
                surf,
                fonts,
                cx + 12,
                icon_box_y,
                Self::CARD_W - 24,
                icon_box_h,
                &item.icon,
                FontFace::UiSemibold,
                18.0,
                icon_fg,
                true,
            );

            // Name label
            let label_y = cy + 56;
            draw_label_aligned(
                surf,
                fonts,
                cx + 6,
                label_y,
                Self::CARD_W - 12,
                18,
                &item.name,
                FontFace::UiRegular,
                12.0,
                theme.text_primary(),
                true,
            );
        }
    }

    /// Hit-test click event against GridView cards. Returns `Some(index)` if card clicked.
    pub fn hit_test(
        click_x: i32,
        click_y: i32,
        x: i32,
        y: i32,
        width: u32,
        count: usize,
    ) -> Option<usize> {
        let cols = ((width.saturating_sub(Self::CARD_GAP)) / (Self::CARD_W + Self::CARD_GAP)).max(1) as usize;
        let pad_x = x + 12;
        let pad_y = y + 12;

        for i in 0..count {
            let col = i % cols;
            let row = i / cols;
            let cx = pad_x + col as i32 * (Self::CARD_W + Self::CARD_GAP) as i32;
            let cy = pad_y + row as i32 * (Self::CARD_H + Self::CARD_GAP) as i32;

            if click_x >= cx && click_x < cx + Self::CARD_W as i32 && click_y >= cy && click_y < cy + Self::CARD_H as i32 {
                return Some(i);
            }
        }
        None
    }
}

// ─── 6. StatusBar Component ──────────────────────────────────────────────────

pub struct StatusBar;

impl StatusBar {
    pub const DEFAULT_HEIGHT: u32 = 24;

    /// Render a clean bottom status bar with top border and status text.
    pub fn render(
        surf: &mut ViSurface,
        fonts: &mut TextFonts,
        theme: &dyn ViTheme,
        y: i32,
        width: u32,
        height: u32,
        message: &str,
    ) {
        // Status bar background
        fill_rect(surf, 0, y, width, height, theme.surface());
        // Top 1px divider
        fill_rect(surf, 0, y, width, 1, theme.border());
        // Message label
        draw_label(
            surf,
            fonts,
            14,
            y + 2,
            width.saturating_sub(28),
            height.saturating_sub(2),
            message,
            FontFace::UiRegular,
            12.0,
            theme.text_secondary(),
        );
    }

    /// Render a split status bar with left message and right status/zoom metrics.
    pub fn render_split(
        surf: &mut ViSurface,
        fonts: &mut TextFonts,
        theme: &dyn ViTheme,
        y: i32,
        width: u32,
        height: u32,
        left_message: &str,
        right_info: &str,
    ) {
        fill_rect(surf, 0, y, width, height, theme.surface());
        fill_rect(surf, 0, y, width, 1, theme.border());

        let right_w = if right_info.is_empty() {
            0
        } else {
            fonts.measure(FontFace::UiRegular, right_info, 12.0) as u32 + 20
        };

        let left_w = width.saturating_sub(right_w + 30);
        draw_label(
            surf,
            fonts,
            14,
            y + 2,
            left_w,
            height.saturating_sub(2),
            left_message,
            FontFace::UiRegular,
            12.0,
            theme.text_secondary(),
        );

        if !right_info.is_empty() {
            let right_x = width.saturating_sub(right_w + 12) as i32;
            draw_label_aligned(
                surf,
                fonts,
                right_x,
                y + 2,
                right_w,
                height.saturating_sub(2),
                right_info,
                FontFace::UiRegular,
                12.0,
                theme.text_secondary(),
                true,
            );
        }
    }
}

// === ScrollBar Component ===

pub struct ScrollBar;

impl ScrollBar {
    pub const DEFAULT_WIDTH: u32 = 12;

    /// Render a standard vertical scrollbar track and thumb.
    pub fn render(
        surf: &mut ViSurface,
        theme: &dyn ViTheme,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        scroll_y: i32,
        max_scroll: i32,
        content_total_h: i32,
    ) {
        // Track
        fill_rect(surf, x, y, width, height, theme.surface());

        if max_scroll > 0 && content_total_h > 0 {
            let thumb_h = ((height as f32 / content_total_h as f32) * height as f32)
                .max(20.0) as u32;
            let scroll_ratio = (scroll_y.max(0) as f32 / max_scroll as f32).clamp(0.0, 1.0);
            let available_track = height.saturating_sub(thumb_h);
            let thumb_y = y + (scroll_ratio * available_track as f32) as i32;

            // Thumb
            fill_rect(surf, x + 2, thumb_y, width.saturating_sub(4), thumb_h, theme.accent());
        }
    }
}

// === ProgressIndicator Component ===

pub struct ProgressIndicator;

impl ProgressIndicator {
    /// Render a thin horizontal progress indicator bar.
    pub fn render(
        surf: &mut ViSurface,
        theme: &dyn ViTheme,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        progress: f32,
    ) {
        let fill_w = (width as f32 * progress.clamp(0.0, 1.0)) as u32;
        if fill_w > 0 {
            fill_rect(surf, x, y, fill_w, height, theme.accent());
        }
    }
}

// === AddressBar Component ===

pub struct AddressBar;

impl AddressBar {
    pub const DEFAULT_HEIGHT: u32 = 28;

    /// Render an editable URL/Search address bar with theme styling.
    pub fn render(
        surf: &mut ViSurface,
        fonts: &mut TextFonts,
        theme: &dyn ViTheme,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        url_text: &str,
    ) {
        fill_rect(surf, x, y, width, height, theme.input_bg());
        stroke_rect(surf, x, y, width, height, theme.border());
        draw_label(
            surf,
            fonts,
            x + 8,
            y,
            width.saturating_sub(16),
            height,
            url_text,
            FontFace::UiRegular,
            14.0,
            theme.text_primary(),
        );
    }
}

// === NavigationButtons Component ===

pub struct NavigationButtons;

impl NavigationButtons {
    /// Render standard Back and Forward browser/viewer navigation buttons.
    pub fn render(
        surf: &mut ViSurface,
        fonts: &mut TextFonts,
        theme: &dyn ViTheme,
        x: i32,
        y: i32,
        can_back: bool,
        can_forward: bool,
    ) {
        for (idx, (label, enabled)) in [("<", can_back), (">", can_forward)].iter().enumerate() {
            let btn_x = x + idx as i32 * 30;
            let (bg, fg) = if *enabled {
                (theme.accent(), Color::WHITE)
            } else {
                (theme.surface(), theme.text_secondary())
            };
            crate::surface::draw_button(surf, fonts, btn_x, y, 26, 28, label, bg, theme.border(), fg);
        }
    }
}
