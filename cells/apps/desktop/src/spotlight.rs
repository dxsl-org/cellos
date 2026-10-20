// SPDX-License-Identifier: MIT
//! Spotlight Search Modal renderer and interaction handler for CellOS Desktop.

extern crate alloc;
use alloc::string::String;

use ostd::display::ViSurface;
use ostd::input::KeySym;
use ostd::typography::{FontFace, TextFonts};

use crate::apps::AppRegistry;
use crate::draw::{draw_button, draw_label, draw_text, fill_rect, stroke_rect, theme, Color};

pub const SPOTLIGHT_WIDTH: u32 = 480;
pub const SPOTLIGHT_HEIGHT: u32 = 320;
pub const MAX_RESULTS: usize = 3;
const TABS_Y: i32 = 56;
const TABS_HEIGHT: u32 = 20;
const LIST_START_Y: i32 = 80;
const ROW_HEIGHT: u32 = 56;
const ROW_STEP: i32 = 62;
const ACTION_WIDTH: u32 = 64;
const ACTION_HEIGHT: u32 = 28;
const ACTION_Y: i32 = (ROW_HEIGHT as i32 - ACTION_HEIGHT as i32) / 2;

fn pin_x(width: u32) -> i32 {
    width as i32 - 164
}

fn run_x(width: u32) -> i32 {
    width as i32 - 92
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpotlightSection {
    #[default]
    Cells = 0,
    ActiveCells = 1,
    System = 2,
}

impl SpotlightSection {
    pub const fn next(self) -> Self {
        match self {
            Self::Cells => Self::ActiveCells,
            Self::ActiveCells => Self::System,
            Self::System => Self::Cells,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotlightAction {
    None,
    Close,
    LaunchApp(&'static str),
    TogglePin(&'static str),
    SwitchTask(usize),
    CycleTheme,
    Reboot,
    ShutDown,
}

pub struct SpotlightState {
    pub is_open: bool,
    pub section: SpotlightSection,
    pub query: String,
    pub selected_index: usize,
    pub mouse_x: i32,
    pub mouse_y: i32,
}

impl SpotlightState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            section: SpotlightSection::Cells,
            query: String::new(),
            selected_index: 0,
            mouse_x: -1,
            mouse_y: -1,
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
        self.section = SpotlightSection::Cells;
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

    // Search input bar
    let input_x = 14;
    let input_y = 10;
    let input_w = sw.saturating_sub(28);
    let input_h = 38;
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
            match state.section {
                SpotlightSection::Cells => "Type cell name to filter...",
                SpotlightSection::ActiveCells => "Browse active cells...",
                SpotlightSection::System => "System settings & power...",
            },
            FontFace::UiRegular, 14.0, theme::TEXT_MUTED, false,
        );
    } else {
        let clip = (query_x, input_y, query_x + query_w as i32, input_y + input_h as i32);
        let query_width = fonts.measure(FontFace::UiRegular, &state.query, 16.0);
        let visible_width = query_w.saturating_sub(3) as f32;
        let offset = (query_width - visible_width).max(0.0);
        draw_text(
            surf, fonts, query_x as f32 - offset, query_y, &state.query,
            FontFace::UiRegular, 16.0, theme::TEXT_PRIMARY, clip,
        );
        let caret_x = query_x + (query_width.min(visible_width) + 0.5) as i32;
        fill_rect(surf, caret_x, input_y + 8, 1, input_h - 16, theme::TEXT_PRIMARY);
    }

    // Three Section Headers: [1 Applications] [2 Tasks] [3 System]
    let tab_w = (sw.saturating_sub(28)) / 3;
    for (i, name) in ["1 Cells", "2 Active", "3 System"].iter().enumerate() {
        let tx = 14 + i as i32 * tab_w as i32;
        let is_active = (i == 0 && state.section == SpotlightSection::Cells)
            || (i == 1 && state.section == SpotlightSection::ActiveCells)
            || (i == 2 && state.section == SpotlightSection::System);
        let tab_col = if is_active { theme::ACCENT_CYAN } else { theme::TEXT_MUTED };
        draw_label(
            surf, fonts, tx, TABS_Y, tab_w, TABS_HEIGHT, name,
            if is_active { FontFace::UiSemibold } else { FontFace::UiRegular },
            13.0, tab_col, true,
        );
        if is_active {
            fill_rect(surf, tx + 8, TABS_Y + TABS_HEIGHT as i32 - 2, tab_w - 16, 2, theme::ACCENT_BLUE);
        }
    }
    fill_rect(surf, 0, TABS_Y + TABS_HEIGHT as i32, sw, 1, theme::MODAL_BORDER);

    // Section Content
    match state.section {
        SpotlightSection::Cells => {
            render_applications(surf, fonts, registry, state, sw);
        }
        SpotlightSection::ActiveCells => {
            render_running_tasks(surf, fonts, state, sw);
        }
        SpotlightSection::System => {
            render_system(surf, fonts, state, sw);
        }
    }

    // Footer
    let footer_y = sh as i32 - 28;
    fill_rect(surf, 0, footer_y, sw, 1, theme::MODAL_BORDER);
    draw_label(
        surf, fonts, 14, footer_y + 1, sw.saturating_sub(28), 26,
        "Tab Switch Section    Esc Close    Enter Select Cell",
        FontFace::UiRegular, 13.0, theme::TEXT_MUTED, false,
    );
    surf.damage_all();
}

fn render_applications(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    registry: &AppRegistry,
    state: &SpotlightState,
    sw: u32,
) {
    let results = registry.filtered(&state.query);
    if results.is_empty() {
        draw_label(
            surf, fonts, 24, LIST_START_Y + 28, sw.saturating_sub(48), 32,
            "No matching cells found", FontFace::UiRegular,
            15.0, theme::TEXT_MUTED, true,
        );
        return;
    }

    let name_height = (fonts.line_height(FontFace::UiSemibold, 15.0) + 1.0) as u32;
    let desc_height = (fonts.line_height(FontFace::UiRegular, 13.0) + 1.0) as u32;

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
        let text_y = row_y + (ROW_HEIGHT as i32 - name_height as i32 - desc_height as i32 - 4) / 2;
        draw_label(
            surf, fonts, text_x, text_y, text_w, name_height, app.name,
            FontFace::UiSemibold, 15.0, theme::TEXT_PRIMARY, false,
        );
        draw_label(
            surf, fonts, text_x, text_y + name_height as i32 + 4,
            text_w, desc_height, app.description,
            FontFace::UiRegular, 13.0, theme::TEXT_MUTED, false,
        );

        let pin_text = if app.is_pinned { "Unpin" } else { "Pin" };
        draw_button(
            surf, fonts, pin_x(sw), row_y + ACTION_Y, ACTION_WIDTH, ACTION_HEIGHT,
            pin_text, theme::BTN_NORMAL, theme::BTN_HOVER, theme::TEXT_PRIMARY,
        );
        draw_button(
            surf, fonts, run_x(sw), row_y + ACTION_Y, ACTION_WIDTH, ACTION_HEIGHT,
            "Open", theme::BTN_NORMAL, theme::BTN_HOVER, theme::TEXT_PRIMARY,
        );
    }
}

fn render_running_tasks(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    state: &SpotlightState,
    sw: u32,
) {
    let tasks: &[(&str, &str, &str)] = &[
        ("(O)", "Ocel Document Viewer", "Active Session (Frame 1)"),
        (">_", "Interactive Terminal", "Active Session (Frame 2)"),
        ("[]", "Desktop Shell", "Background Workspace"),
    ];

    let name_height = (fonts.line_height(FontFace::UiSemibold, 15.0) + 1.0) as u32;
    let desc_height = (fonts.line_height(FontFace::UiRegular, 13.0) + 1.0) as u32;

    for (i, &(icon, name, status)) in tasks.iter().take(MAX_RESULTS).enumerate() {
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
            surf, fonts, 24, row_y, 34, ROW_HEIGHT, icon,
            FontFace::UiRegular, 14.0, theme::ACCENT_CYAN, true,
        );

        let text_x = 70;
        let text_w = (run_x(sw) - 10 - text_x).max(0) as u32;
        let text_y = row_y + (ROW_HEIGHT as i32 - name_height as i32 - desc_height as i32 - 4) / 2;
        draw_label(
            surf, fonts, text_x, text_y, text_w, name_height, name,
            FontFace::UiSemibold, 15.0, theme::TEXT_PRIMARY, false,
        );
        draw_label(
            surf, fonts, text_x, text_y + name_height as i32 + 4,
            text_w, desc_height, status,
            FontFace::UiRegular, 13.0, theme::TEXT_MUTED, false,
        );

        draw_button(
            surf, fonts, run_x(sw), row_y + ACTION_Y, ACTION_WIDTH, ACTION_HEIGHT,
            "Switch", theme::BTN_NORMAL, theme::BTN_HOVER, theme::TEXT_PRIMARY,
        );
    }
}

fn render_system(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    state: &SpotlightState,
    sw: u32,
) {
    let actions: &[(&str, &str, &str, &str)] = &[
        ("[T]", "Appearance Theme", "Cycle Light / Dark / Paper", "Cycle"),
        ("[R]", "Reboot System", "Authorized restart after session consent", "Reboot"),
        ("[P]", "Power Off", "Safely shut down the computer", "Shut Down"),
    ];

    let name_height = (fonts.line_height(FontFace::UiSemibold, 15.0) + 1.0) as u32;
    let desc_height = (fonts.line_height(FontFace::UiRegular, 13.0) + 1.0) as u32;

    for (i, &(icon, name, desc, btn_text)) in actions.iter().take(MAX_RESULTS).enumerate() {
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
            surf, fonts, 24, row_y, 34, ROW_HEIGHT, icon,
            FontFace::UiRegular, 14.0, theme::ACCENT_CYAN, true,
        );

        let text_x = 70;
        let text_w = (run_x(sw) - 10 - text_x).max(0) as u32;
        let text_y = row_y + (ROW_HEIGHT as i32 - name_height as i32 - desc_height as i32 - 4) / 2;
        draw_label(
            surf, fonts, text_x, text_y, text_w, name_height, name,
            FontFace::UiSemibold, 15.0, theme::TEXT_PRIMARY, false,
        );
        draw_label(
            surf, fonts, text_x, text_y + name_height as i32 + 4,
            text_w, desc_height, desc,
            FontFace::UiRegular, 13.0, theme::TEXT_MUTED, false,
        );

        draw_button(
            surf, fonts, run_x(sw), row_y + ACTION_Y, ACTION_WIDTH, ACTION_HEIGHT,
            btn_text, theme::BTN_NORMAL, theme::BTN_HOVER, theme::TEXT_PRIMARY,
        );
    }
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
        KeySym::Tab => {
            state.section = state.section.next();
            state.selected_index = 0;
            SpotlightAction::None
        }
        KeySym::Return => {
            match state.section {
                SpotlightSection::Cells => {
                    let results = registry.filtered(&state.query);
                    if let Some(app) = results.get(state.selected_index) {
                        let path = app.path;
                        state.close();
                        SpotlightAction::LaunchApp(path)
                    } else {
                        SpotlightAction::None
                    }
                }
                SpotlightSection::ActiveCells => {
                    let idx = state.selected_index;
                    state.close();
                    SpotlightAction::SwitchTask(idx)
                }
                SpotlightSection::System => {
                    match state.selected_index {
                        0 => SpotlightAction::CycleTheme,
                        1 => SpotlightAction::Reboot,
                        2 => SpotlightAction::ShutDown,
                        _ => SpotlightAction::None,
                    }
                }
            }
        }
        KeySym::Up => {
            if state.selected_index > 0 {
                state.selected_index -= 1;
            }
            SpotlightAction::None
        }
        KeySym::Down => {
            let max_idx = match state.section {
                SpotlightSection::Cells => registry.filtered(&state.query).len().min(MAX_RESULTS),
                SpotlightSection::ActiveCells | SpotlightSection::System => 3,
            };
            if max_idx > 0 && state.selected_index + 1 < max_idx {
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
    // Check section tabs
    if y >= TABS_Y && y < TABS_Y + TABS_HEIGHT as i32 {
        let tab_w = (sw.saturating_sub(28)) / 3;
        let tab_idx = ((x - 14).max(0) as u32) / tab_w;
        state.section = match tab_idx {
            0 => SpotlightSection::Cells,
            1 => SpotlightSection::ActiveCells,
            _ => SpotlightSection::System,
        };
        state.selected_index = 0;
        return SpotlightAction::None;
    }

    match state.section {
        SpotlightSection::Cells => {
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

                // Check Open button or row click
                if is_inside(x, y, run_x(sw), pin_y, ACTION_WIDTH, ACTION_HEIGHT)
                    || is_inside(x, y, 14, row_y, sw - 28, row_h as u32)
                {
                    state.selected_index = i;
                    let path = app.path;
                    state.close();
                    return SpotlightAction::LaunchApp(path);
                }
            }
        }
        SpotlightSection::ActiveCells => {
            for i in 0..MAX_RESULTS {
                let row_y = LIST_START_Y + i as i32 * ROW_STEP;
                if is_inside(x, y, 14, row_y, sw - 28, ROW_HEIGHT) {
                    state.selected_index = i;
                    state.close();
                    return SpotlightAction::SwitchTask(i);
                }
            }
        }
        SpotlightSection::System => {
            for i in 0..MAX_RESULTS {
                let row_y = LIST_START_Y + i as i32 * ROW_STEP;
                if is_inside(x, y, 14, row_y, sw - 28, ROW_HEIGHT) {
                    state.selected_index = i;
                    return match i {
                        0 => SpotlightAction::CycleTheme,
                        1 => SpotlightAction::Reboot,
                        2 => SpotlightAction::ShutDown,
                        _ => SpotlightAction::None,
                    };
                }
            }
        }
    }

    SpotlightAction::None
}

fn is_inside(mx: i32, my: i32, x: i32, y: i32, w: u32, h: u32) -> bool {
    mx >= x && mx < x + w as i32 && my >= y && my < y + h as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spotlight_section_cycling_and_actions() {
        let registry = AppRegistry::new();
        let mut state = SpotlightState::new();
        state.open();

        assert_eq!(state.section, SpotlightSection::Cells);

        // Tab key cycles section to RunningTasks
        let act1 = handle_key(&registry, &mut state, KeySym::Tab, None);
        assert_eq!(act1, SpotlightAction::None);
        assert_eq!(state.section, SpotlightSection::ActiveCells);

        // Enter on RunningTasks switches task
        let act_task = handle_key(&registry, &mut state, KeySym::Return, None);
        assert_eq!(act_task, SpotlightAction::SwitchTask(0));

        // Re-open and Tab to System
        state.open();
        handle_key(&registry, &mut state, KeySym::Tab, None); // to RunningTasks
        handle_key(&registry, &mut state, KeySym::Tab, None); // to System
        assert_eq!(state.section, SpotlightSection::System);

        // First action in System is CycleTheme
        let act_theme = handle_key(&registry, &mut state, KeySym::Return, None);
        assert_eq!(act_theme, SpotlightAction::CycleTheme);

        // Move down to Reboot
        handle_key(&registry, &mut state, KeySym::Down, None);
        assert_eq!(state.selected_index, 1);
        let act_reboot = handle_key(&registry, &mut state, KeySym::Return, None);
        assert_eq!(act_reboot, SpotlightAction::Reboot);

        // Move down to ShutDown
        handle_key(&registry, &mut state, KeySym::Down, None);
        assert_eq!(state.selected_index, 2);
        let act_shutdown = handle_key(&registry, &mut state, KeySym::Return, None);
        assert_eq!(act_shutdown, SpotlightAction::ShutDown);

        // Escape closes
        let act_esc = handle_key(&registry, &mut state, KeySym::Escape, None);
        assert_eq!(act_esc, SpotlightAction::Close);
        assert!(!state.is_open);
    }
}
