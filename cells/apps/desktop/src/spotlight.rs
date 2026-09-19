// SPDX-License-Identifier: MIT
//! Spotlight Search Modal renderer and interaction handler for CellOS Desktop.

extern crate alloc;
use alloc::string::String;

use ostd::display::ViSurface;
use ostd::input::KeySym;

use crate::apps::AppRegistry;
use crate::draw::{draw_button, draw_str, fill_rect, stroke_rect, theme, Color};

pub const SPOTLIGHT_WIDTH: u32 = 500;
pub const SPOTLIGHT_HEIGHT: u32 = 280;
pub const MAX_RESULTS: usize = 4;

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

pub fn render(surf: &mut ViSurface, registry: &AppRegistry, state: &SpotlightState) {
    let sw = surf.width();
    let sh = surf.height();

    // ── 1. Modal Background & Outer Frame ─────────────────────────────────────
    fill_rect(surf, 0, 0, sw, sh, theme::MODAL_BG);
    stroke_rect(surf, 0, 0, sw, sh, theme::MODAL_BORDER);

    // ── 2. Header: Search Input Box ───────────────────────────────────────────
    let input_x = 14i32;
    let input_y = 12i32;
    let input_w = sw - 28;
    let input_h = 34u32;
    fill_rect(
        surf,
        input_x,
        input_y,
        input_w,
        input_h,
        Color::rgb(32, 35, 45),
    );
    stroke_rect(surf, input_x, input_y, input_w, input_h, theme::ACCENT_BLUE);

    // Icon prefix
    draw_str(surf, input_x + 10, input_y + 13, "?", theme::ACCENT_CYAN, 1);

    // Search Query
    if state.query.is_empty() {
        draw_str(
            surf,
            input_x + 28,
            input_y + 13,
            "Type app name or description...",
            theme::TEXT_MUTED,
            1,
        );
    } else {
        let mut display_query = state.query.clone();
        display_query.push('_'); // blinking cursor indicator
        draw_str(
            surf,
            input_x + 28,
            input_y + 13,
            &display_query,
            theme::TEXT_PRIMARY,
            1,
        );
    }

    // Divider line below search bar
    fill_rect(surf, 0, 54, sw, 1, theme::MODAL_BORDER);

    // ── 3. Filtered Results List ──────────────────────────────────────────────
    let results = registry.filtered(&state.query);
    let total_results = results.len();

    let list_start_y = 60i32;
    let row_h = 44u32;

    if results.is_empty() {
        draw_str(
            surf,
            24,
            list_start_y + 40,
            "No matching applications found",
            theme::TEXT_MUTED,
            1,
        );
    } else {
        let max_display = total_results.min(MAX_RESULTS);
        for (i, app) in results.iter().take(max_display).enumerate() {
            let row_y = list_start_y + (i as i32 * (row_h as i32 + 4));
            let is_selected = i == state.selected_index;
            let is_hover = is_inside(state.mouse_x, state.mouse_y, 14, row_y, sw - 28, row_h);

            let row_bg = if is_selected {
                theme::BTN_ACTIVE
            } else if is_hover {
                theme::BTN_HOVER
            } else {
                theme::BTN_NORMAL
            };

            // Row container
            fill_rect(surf, 14, row_y, sw - 28, row_h, row_bg);
            if is_selected {
                fill_rect(surf, 14, row_y, 4, row_h, theme::ACCENT_CYAN); // accent strip
                stroke_rect(surf, 14, row_y, sw - 28, row_h, theme::ACCENT_BLUE);
            } else {
                stroke_rect(surf, 14, row_y, sw - 28, row_h, theme::TASKBAR_BORDER);
            }

            // Icon badge
            draw_str(surf, 26, row_y + 18, app.icon, theme::ACCENT_CYAN, 1);

            // App Name & Description
            draw_str(surf, 58, row_y + 10, app.name, theme::TEXT_PRIMARY, 1);
            draw_str(surf, 58, row_y + 24, app.description, theme::TEXT_MUTED, 1);

            // ── Pin / Unpin Button ──
            let pin_x = sw as i32 - 146;
            let pin_y = row_y + 9;
            let (pin_text, pin_color) = if app.is_pinned {
                ("Unpin", theme::ACCENT_RED)
            } else {
                ("+Pin", theme::ACCENT_GREEN)
            };
            let pin_hover = is_inside(state.mouse_x, state.mouse_y, pin_x, pin_y, 56, 26);
            let pin_bg = if pin_hover {
                theme::BTN_HOVER
            } else {
                Color::rgb(28, 30, 38)
            };
            draw_button(
                surf, pin_x, pin_y, 56, 26, pin_text, pin_bg, pin_color, pin_color,
            );

            // ── Run Button ──
            let run_x = sw as i32 - 82;
            let run_y = row_y + 9;
            let run_hover = is_inside(state.mouse_x, state.mouse_y, run_x, run_y, 60, 26);
            let run_bg = if run_hover {
                theme::ACCENT_BLUE
            } else {
                theme::BTN_NORMAL
            };
            draw_button(
                surf,
                run_x,
                run_y,
                60,
                26,
                "Open",
                run_bg,
                theme::ACCENT_CYAN,
                theme::TEXT_PRIMARY,
            );
        }
    }

    // ── 4. Footer: Keyboard & Action Hints ────────────────────────────────────
    let footer_y = sh as i32 - 24;
    fill_rect(surf, 0, footer_y - 2, sw, 1, theme::MODAL_BORDER);
    draw_str(
        surf,
        14,
        footer_y + 4,
        "[ESC] Close   [ENTER] Open   [UP/DN] Navigate",
        theme::TEXT_MUTED,
        1,
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

    let list_start_y = 60i32;
    let row_h = 44i32;

    for (i, app) in results.iter().take(max_display).enumerate() {
        let row_y = list_start_y + (i as i32 * (row_h + 4));

        // Check Pin button
        let pin_x = sw as i32 - 146;
        let pin_y = row_y + 9;
        if is_inside(x, y, pin_x, pin_y, 56, 26) {
            let id = app.id;
            registry.toggle_pin(id);
            return SpotlightAction::TogglePin(id);
        }

        // Check Run button
        let run_x = sw as i32 - 82;
        let run_y = row_y + 9;
        if is_inside(x, y, run_x, run_y, 60, 26) {
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
