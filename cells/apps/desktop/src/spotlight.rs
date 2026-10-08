// SPDX-License-Identifier: MIT
//! Spotlight Search Modal renderer and interaction handler for CellOS Desktop.

extern crate alloc;
use alloc::string::String;

use ostd::display::ViSurface;
use ostd::input::KeySym;
use ostd::typography::{FontFace, TextFonts};

use crate::apps::AppRegistry;
use crate::draw::{draw_button, draw_label, draw_text, fill_rect, stroke_rect, theme, Color};

pub const SPOTLIGHT_WIDTH: u32 = 600;
pub const SPOTLIGHT_HEIGHT: u32 = 392;
pub const MAX_RESULTS: usize = 4;
const LIST_START_Y: i32 = 76;
const ROW_HEIGHT: u32 = 64;
const ROW_STEP: i32 = ROW_HEIGHT as i32 + 6;
const ACTION_WIDTH: u32 = 64;
const ACTION_HEIGHT: u32 = 32;
const ACTION_Y: i32 = (ROW_HEIGHT as i32 - ACTION_HEIGHT as i32) / 2;

fn pin_x(width: u32) -> i32 {
    width as i32 - 164
}

fn run_x(width: u32) -> i32 {
    width as i32 - 92
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SpotlightAction {
    None,
    Close,
    LaunchApp(&'static str),
    TogglePin(&'static str),
}

pub struct SpotlightState {
    pub is_open: bool,
    pub query: String,
    pub selected_index: usize,
    pub mouse_x: i32,
    pub mouse_y: i32,
}

impl SpotlightState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            query: String::new(),
            selected_index: 0,
            mouse_x: -1,
            mouse_y: -1,
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
        self.query.clear();
        self.selected_index = 0;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.query.clear();
        self.selected_index = 0;
    }
}

pub fn render(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    registry: &AppRegistry,
    state: &SpotlightState,
) {
    let sw = surf.width();
    let sh = surf.height();
    fill_rect(surf, 0, 0, sw, sh, theme::MODAL_BG);
    stroke_rect(surf, 0, 0, sw, sh, theme::MODAL_BORDER);

    let input_x = 14;
    let input_y = 12;
    let input_w = sw.saturating_sub(28);
    let input_h = 44;
    fill_rect(surf, input_x, input_y, input_w, input_h, Color::rgb(32, 35, 45));
    stroke_rect(surf, input_x, input_y, input_w, input_h, theme::ACCENT_BLUE);
    draw_label(
        surf, fonts, input_x + 8, input_y, 22, input_h, "?",
        FontFace::UiSemibold, 16.0, theme::ACCENT_CYAN, true,
    );
    let query_x = input_x + 36;
    let query_w = input_w.saturating_sub(48);
    let query_y = input_y as f32 + (input_h as f32
        - fonts.line_height(FontFace::UiRegular, 16.0)) / 2.0;
    if state.query.is_empty() {
        draw_label(
            surf, fonts, query_x, input_y, query_w, input_h,
            "Type app name or description...", FontFace::UiRegular,
            16.0, theme::TEXT_MUTED, false,
        );
    } else {
        let clip = (query_x, input_y, query_x + query_w as i32, input_y + input_h as i32);
        let query_width = fonts.measure(FontFace::UiRegular, &state.query, 16.0);
        // Scroll the input horizontally without cloning or splitting Unicode text.
        let visible_width = query_w.saturating_sub(3) as f32;
        let offset = (query_width - visible_width).max(0.0);
        draw_text(
            surf, fonts, query_x as f32 - offset, query_y, &state.query,
            FontFace::UiRegular, 16.0, theme::TEXT_PRIMARY, clip,
        );
        let caret_x = query_x + (query_width.min(visible_width) + 0.5) as i32;
        fill_rect(surf, caret_x, input_y + 12, 1, input_h - 24, theme::TEXT_PRIMARY);
    }
    fill_rect(surf, 0, 68, sw, 1, theme::MODAL_BORDER);

    let results = registry.filtered(&state.query);
    if results.is_empty() {
        draw_label(
            surf, fonts, 24, LIST_START_Y + 28, sw.saturating_sub(48), 32,
            "No matching applications found", FontFace::UiRegular,
            16.0, theme::TEXT_MUTED, true,
        );
    } else {
        // Round line boxes outward so fractional metrics cannot clip their bottom.
        let name_height = (fonts.line_height(FontFace::UiSemibold, 16.0) + 1.0) as u32;
        let description_height = (fonts.line_height(FontFace::UiRegular, 14.0) + 1.0) as u32;
        for (i, app) in results.iter().take(MAX_RESULTS).enumerate() {
            let row_y = LIST_START_Y + i as i32 * ROW_STEP;
            let row_w = sw.saturating_sub(28);
            let is_selected = i == state.selected_index;
            let is_hover = is_inside(state.mouse_x, state.mouse_y, 14, row_y, row_w, ROW_HEIGHT);
            let row_bg = if is_selected {
                theme::BTN_ACTIVE
            } else if is_hover {
                theme::BTN_HOVER
            } else {
                theme::BTN_NORMAL
            };
            fill_rect(surf, 14, row_y, row_w, ROW_HEIGHT, row_bg);
            if is_selected {
                fill_rect(surf, 14, row_y, 4, ROW_HEIGHT, theme::ACCENT_CYAN);
            }
            stroke_rect(
                surf, 14, row_y, row_w, ROW_HEIGHT,
                if is_selected { theme::ACCENT_BLUE } else { theme::TASKBAR_BORDER },
            );
            draw_label(
                surf, fonts, 24, row_y, 34, ROW_HEIGHT, app.icon,
                FontFace::UiRegular, 14.0, theme::ACCENT_CYAN, true,
            );

            let text_x = 70;
            let text_w = (pin_x(sw) - 10 - text_x).max(0) as u32;
            let text_y = row_y + (ROW_HEIGHT as i32
                - name_height as i32 - description_height as i32 - 4) / 2;
            draw_label(
                surf, fonts, text_x, text_y, text_w, name_height, app.name,
                FontFace::UiSemibold, 16.0, theme::TEXT_PRIMARY, false,
            );
            draw_label(
                surf, fonts, text_x, text_y + name_height as i32 + 4,
                text_w, description_height, app.description,
                FontFace::UiRegular, 14.0, theme::TEXT_MUTED, false,
            );

            let pin_y = row_y + ACTION_Y;
            let (pin_text, pin_color) = if app.is_pinned {
                ("Unpin", theme::ACCENT_RED)
            } else {
                ("+Pin", theme::ACCENT_GREEN)
            };
            let pin_bg = if is_inside(
                state.mouse_x, state.mouse_y, pin_x(sw), pin_y, ACTION_WIDTH, ACTION_HEIGHT,
            ) {
                theme::BTN_HOVER
            } else {
                Color::rgb(28, 30, 38)
            };
            draw_button(
                surf, fonts, pin_x(sw), pin_y, ACTION_WIDTH, ACTION_HEIGHT,
                pin_text, pin_bg, pin_color, pin_color,
            );
            let run_bg = if is_inside(
                state.mouse_x, state.mouse_y, run_x(sw), pin_y, ACTION_WIDTH, ACTION_HEIGHT,
            ) {
                theme::ACCENT_BLUE
            } else {
                theme::BTN_NORMAL
            };
            draw_button(
                surf, fonts, run_x(sw), pin_y, ACTION_WIDTH, ACTION_HEIGHT,
                "Open", run_bg, theme::ACCENT_CYAN, theme::TEXT_PRIMARY,
            );
        }
    }

    let footer_y = sh as i32 - 28;
    fill_rect(surf, 0, footer_y, sw, 1, theme::MODAL_BORDER);
    draw_label(
        surf, fonts, 14, footer_y + 1, sw.saturating_sub(28), 26,
        "Esc Close    Enter Open    Up / Down Navigate",
        FontFace::UiRegular, 14.0, theme::TEXT_MUTED, false,
    );
    surf.damage_all();
}

pub fn handle_key(
    registry: &AppRegistry,
    state: &mut SpotlightState,
    key_sym: KeySym,
    char_opt: Option<char>,
) -> SpotlightAction {
    match key_sym {
        KeySym::Escape => {
            state.close();
            SpotlightAction::Close
        }
        KeySym::Return => {
            let results = registry.filtered(&state.query);
            if let Some(app) = results.get(state.selected_index) {
                let path = app.path;
                state.close();
                SpotlightAction::LaunchApp(path)
            } else {
                SpotlightAction::None
            }
        }
        KeySym::Up => {
            if state.selected_index > 0 {
                state.selected_index -= 1;
            }
            SpotlightAction::None
        }
        KeySym::Down => {
            let count = registry.filtered(&state.query).len().min(MAX_RESULTS);
            if count > 0 && state.selected_index + 1 < count {
                state.selected_index += 1;
            }
            SpotlightAction::None
        }
        KeySym::Backspace => {
            state.query.pop();
            state.selected_index = 0;
            SpotlightAction::None
        }
        _ => {
            if let Some(c) = char_opt {
                if !c.is_control() {
                    state.query.push(c);
                    state.selected_index = 0;
                }
            }
            SpotlightAction::None
        }
    }
}

pub fn handle_click(
    registry: &mut AppRegistry,
    state: &mut SpotlightState,
    x: i32,
    y: i32,
    sw: u32,
) -> SpotlightAction {
    let results = registry.filtered(&state.query);
    let total_results = results.len();
    let max_display = total_results.min(MAX_RESULTS);

    let row_h = ROW_HEIGHT as i32;

    for (i, app) in results.iter().take(max_display).enumerate() {
        let row_y = LIST_START_Y + i as i32 * ROW_STEP;

        // Check Pin button
        let pin_y = row_y + ACTION_Y;
        if is_inside(x, y, pin_x(sw), pin_y, ACTION_WIDTH, ACTION_HEIGHT) {
            let id = app.id;
            registry.toggle_pin(id);
            return SpotlightAction::TogglePin(id);
        }

        // Check Run button
        if is_inside(x, y, run_x(sw), pin_y, ACTION_WIDTH, ACTION_HEIGHT) {
            let path = app.path;
            state.close();
            return SpotlightAction::LaunchApp(path);
        }

        // Check Row click (selects and launches)
        if is_inside(x, y, 14, row_y, sw - 28, row_h as u32) {
            state.selected_index = i;
            let path = app.path;
            state.close();
            return SpotlightAction::LaunchApp(path);
        }
    }

    SpotlightAction::None
}

fn is_inside(mx: i32, my: i32, x: i32, y: i32, w: u32, h: u32) -> bool {
    mx >= x && mx < x + w as i32 && my >= y && my < y + h as i32
}
