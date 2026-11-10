// SPDX-License-Identifier: MIT
//! Frame rendering: toolbar, Places, one listing (or two side by side when split),
//! status bar and Quick Look. Geometry comes from [`crate::layout`], which mouse
//! hit-testing uses too.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use viui::canvas::Color;
use viui::surface::components::{
    GridItemDef, GridView, ListView, ListViewItemDef, PlacesItemDef, PlacesSidebar,
    SegmentedControl, StatusBar, Toolbar,
};
use viui::theme::{ViTheme, LIGHT_THEME};

use crate::app::FilesApp;
use crate::draw::{draw_label, fill_rect, stroke_rect};
use crate::fs::FileSystem;
use crate::layout::{self, Segments, PANE_HEADER_HEIGHT, STATUS_HEIGHT, TOOLBAR_HEIGHT};
use crate::model::{FileItem, ViewMode};
use crate::preview;
use crate::split::PaneView;
use crate::views::{self, places::PLACES_WIDTH, ContentRect};

impl<F: FileSystem> FilesApp<F> {
    /// Draw the whole window and mark it damaged.
    pub fn render(&self, surf: &mut ViSurface, fonts: &mut TextFonts) {
        let sw = self.width;
        let sh = self.height;
        let theme = &LIGHT_THEME;

        let path_title = format!("Files: {}", self.current_path);
        Toolbar::render(surf, fonts, theme, sw, TOOLBAR_HEIGHT, "[F]", &path_title);
        let mode_idx = match self.view_mode {
            ViewMode::List => 0,
            ViewMode::Grid => 1,
            ViewMode::Columns => 2,
        };
        segments(
            surf,
            fonts,
            layout::view_switch(sw),
            &layout::VIEW_MODE_LABELS,
            mode_idx,
        );
        // Lit while split, like the selected view mode; no index lights none.
        let split_idx = if self.is_split() { 0 } else { usize::MAX };
        segments(
            surf,
            fonts,
            layout::split_button(sw),
            &[layout::SPLIT_LABEL],
            split_idx,
        );

        let content = layout::content_rect(sw, sh);
        let places_defs: Vec<PlacesItemDef> = self
            .places
            .iter()
            .map(|p| PlacesItemDef {
                name: String::from(p.name),
                path: String::from(p.path),
                icon: String::from(p.icon),
            })
            .collect();
        PlacesSidebar::render(
            surf,
            fonts,
            theme,
            0,
            content.y,
            PLACES_WIDTH,
            content.h,
            "PLACES",
            &places_defs,
            &self.current_path,
        );

        let (left, right) = self.pane_views();
        match (right, layout::pane_frames(sw, sh, right.is_some())) {
            (Some(right), (left_frame, Some(right_frame))) => {
                render_pane(surf, fonts, self.view_mode, &left, left_frame);
                let divider_x = left_frame.x + left_frame.w as i32;
                let divider_w = (right_frame.x - divider_x) as u32;
                fill_rect(
                    surf,
                    divider_x,
                    content.y,
                    divider_w,
                    content.h,
                    theme.border(),
                );
                render_pane(surf, fonts, self.view_mode, &right, right_frame);
            }
            _ => render_listing(
                surf,
                fonts,
                self.view_mode,
                left.items,
                left.selected_index,
                content,
            ),
        }

        let status_y = sh as i32 - STATUS_HEIGHT as i32;
        StatusBar::render(
            surf,
            fonts,
            theme,
            status_y,
            sw,
            STATUS_HEIGHT,
            &self.status_message,
        );
        preview::render(surf, fonts, &self.quick_look, sw, sh);
        surf.damage_all();
    }
}

fn segments(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    at: Segments,
    labels: &[&str],
    selected: usize,
) {
    SegmentedControl::render(
        surf,
        fonts,
        &LIGHT_THEME,
        at.x,
        at.y,
        at.item_w,
        at.h,
        labels,
        selected,
    );
}

/// One split pane: a header strip naming its folder (accent-filled when focused), its
/// listing below, and an accent outline around the focused pane.
fn render_pane(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    mode: ViewMode,
    pane: &PaneView<'_>,
    frame: ContentRect,
) {
    let theme = &LIGHT_THEME;
    let (header_bg, header_fg) = if pane.focused {
        (theme.accent(), Color::WHITE)
    } else {
        (theme.surface(), theme.text_secondary())
    };
    fill_rect(
        surf,
        frame.x,
        frame.y,
        frame.w,
        PANE_HEADER_HEIGHT,
        header_bg,
    );
    draw_label(
        surf,
        fonts,
        frame.x + 10,
        frame.y + 3,
        frame.w.saturating_sub(20),
        18,
        pane.path,
        FontFace::UiSemibold,
        13.0,
        header_fg,
    );
    let listing = layout::listing_rect(frame, true);
    render_listing(surf, fonts, mode, pane.items, pane.selected_index, listing);
    if pane.focused {
        stroke_rect(surf, frame.x, frame.y, frame.w, frame.h, theme.accent());
    }
}

/// The entries of one folder in the current view mode.
fn render_listing(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    mode: ViewMode,
    items: &[FileItem],
    selected: usize,
    rect: ContentRect,
) {
    let theme = &LIGHT_THEME;
    match mode {
        ViewMode::List => {
            let defs: Vec<ListViewItemDef> = items
                .iter()
                .map(|item| ListViewItemDef {
                    icon: String::from(item.icon),
                    name: item.name.clone(),
                    size_str: item.formatted_size(),
                    kind_str: String::from(if item.is_dir { "Folder" } else { "File" }),
                    is_dir: item.is_dir,
                })
                .collect();
            ListView::render(
                surf,
                fonts,
                theme,
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                &defs,
                Some(selected),
            );
        }
        ViewMode::Grid => {
            let defs: Vec<GridItemDef> = items
                .iter()
                .map(|item| GridItemDef {
                    icon: String::from(item.icon),
                    name: item.name.clone(),
                    is_dir: item.is_dir,
                })
                .collect();
            GridView::render(
                surf,
                fonts,
                theme,
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                &defs,
                Some(selected),
            );
        }
        ViewMode::Columns => views::columns::render(
            surf,
            fonts,
            items,
            selected,
            rect,
            views::ViewPalette {
                bg: theme.bg(),
                fg: theme.text_primary(),
                accent: theme.accent(),
                border: theme.border(),
            },
        ),
    }
}
