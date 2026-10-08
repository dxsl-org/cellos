// SPDX-License-Identifier: MIT
//! Bottom Taskbar renderer and interaction handler for CellOS Desktop.

use api::syscall::service;
use ostd::display::ViSurface;
use ostd::syscall::{sys_get_time, sys_lookup_service};
use ostd::typography::{FontFace, TextFonts};

use crate::apps::AppRegistry;
use crate::draw::{draw_button, draw_label, fill_rect, stroke_rect, theme};

pub const TASKBAR_HEIGHT: u32 = 48;
pub const MAX_VISIBLE_APPS: usize = 4;
const BUTTON_Y: i32 = 6;
const BUTTON_HEIGHT: u32 = 36;
const APPS_X: i32 = 204;
const APP_WIDTH: u32 = 148;
const APP_STEP: i32 = APP_WIDTH as i32 + 6;
const PAGE_WIDTH: u32 = 28;
const PAGE_STEP: i32 = 34;
const MORE_WIDTH: u32 = 64;

/// Shared by painting and hit testing, including narrow-screen pagination.
struct Layout {
    tray_x: i32,
    start: usize,
    end: usize,
    capacity: usize,
    paged: bool,
}

impl Layout {
    fn new(width: u32, total: usize, offset: usize) -> Self {
        let tray_width = if width >= 600 { 212 } else { 64 };
        let tray_x = (width as i32 - tray_width - 8).max(APPS_X);
        let available = (tray_x - 12 - APPS_X).max(0);
        // Reserve both arrows and More so page changes never move into the tray.
        let capacity = ((available - 2 * PAGE_STEP - MORE_WIDTH as i32 - 6).max(0)
            / APP_STEP) as usize;
        let capacity = capacity.min(MAX_VISIBLE_APPS);
        let start = if capacity == 0 {
            0
        } else {
            offset.min(total.saturating_sub(1)) / capacity * capacity
        };
        Self {
            tray_x,
            start,
            end: (start + capacity).min(total),
            capacity,
            paged: total > capacity,
        }
    }

    fn apps_x(&self) -> i32 {
        APPS_X + if self.paged { PAGE_STEP } else { 0 }
    }

    fn next_x(&self) -> i32 {
        self.apps_x() + (self.end - self.start) as i32 * APP_STEP
    }

    fn more_x(&self) -> i32 {
        self.tray_x - 12 - MORE_WIDTH as i32
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TaskbarAction {
    None,
    OpenSpotlight,
    LaunchApp(&'static str),
    ToggleSubTaskbar,
}

pub struct TaskbarState {
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub page_offset: usize,
    pub sub_taskbar_open: bool,
    pub last_clock_min: u32,
    pub clock_buf: [u8; 5],
    pub net_online: bool,
    pub vfs_online: bool,
    pub ai_online: bool,
}

impl TaskbarState {
    pub fn new() -> Self {
        Self {
            mouse_x: -1,
            mouse_y: -1,
            page_offset: 0,
            sub_taskbar_open: false,
            last_clock_min: u32::MAX,
            clock_buf: *b"12:00",
            net_online: false,
            vfs_online: false,
            ai_online: false,
        }
    }

    pub fn update_daemons(&mut self) {
        self.net_online = sys_lookup_service(service::NET).is_some();
        self.vfs_online = sys_lookup_service(service::VFS).is_some();
        self.ai_online = sys_lookup_service(15 /* AI */).is_some();

        // Calculate time from monotonic ticks:
        let ticks = sys_get_time();
        let total_secs = ticks / 10_000_000;
        let mins = ((total_secs / 60) % 60) as u32;
        let hours = ((total_secs / 3600) % 24) as u32;

        self.clock_buf[0] = b'0' + (hours / 10) as u8;
        self.clock_buf[1] = b'0' + (hours % 10) as u8;
        self.clock_buf[2] = b':';
        self.clock_buf[3] = b'0' + (mins / 10) as u8;
        self.clock_buf[4] = b'0' + (mins % 10) as u8;
        self.last_clock_min = mins;
    }
}

pub fn render(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    registry: &AppRegistry,
    state: &TaskbarState,
) {
    let sw = surf.width();
    let sh = surf.height();
    let pinned = registry.pinned_apps();
    let layout = Layout::new(sw, pinned.len(), state.page_offset);

    fill_rect(surf, 0, 0, sw, sh, theme::TASKBAR_BG);
    fill_rect(surf, 0, 0, sw, 1, theme::TASKBAR_BORDER);

    for (x, w, text, border, fg) in [
        (8, 84, "CellOS", theme::TASKBAR_BORDER, theme::ACCENT_CYAN),
        (98, 94, "Search", theme::ACCENT_BLUE, theme::TEXT_PRIMARY),
    ] {
        let bg = if is_inside(state.mouse_x, state.mouse_y, x, BUTTON_Y, w, BUTTON_HEIGHT) {
            theme::BTN_HOVER
        } else {
            theme::BTN_NORMAL
        };
        draw_button(surf, fonts, x, BUTTON_Y, w, BUTTON_HEIGHT, text, bg, border, fg);
    }
    fill_rect(surf, 198, 10, 1, 28, theme::TASKBAR_BORDER);

    if layout.paged && layout.start > 0 {
        render_page_button(surf, fonts, state, APPS_X, "<");
    }
    let mut cursor_x = layout.apps_x();
    for app in &pinned[layout.start..layout.end] {
        let bg = if is_inside(
            state.mouse_x, state.mouse_y, cursor_x, BUTTON_Y, APP_WIDTH, BUTTON_HEIGHT,
        ) {
            theme::BTN_HOVER
        } else {
            theme::BTN_NORMAL
        };
        fill_rect(surf, cursor_x, BUTTON_Y, APP_WIDTH, BUTTON_HEIGHT, bg);
        stroke_rect(surf, cursor_x, BUTTON_Y, APP_WIDTH, BUTTON_HEIGHT, theme::TASKBAR_BORDER);
        draw_label(
            surf, fonts, cursor_x + 6, BUTTON_Y, 28, BUTTON_HEIGHT, app.icon,
            FontFace::UiRegular, 14.0, theme::ACCENT_CYAN, true,
        );
        draw_label(
            surf, fonts, cursor_x + 40, BUTTON_Y, APP_WIDTH - 48, BUTTON_HEIGHT,
            app.name, FontFace::UiRegular, 14.0, theme::TEXT_PRIMARY, false,
        );
        cursor_x += APP_STEP;
    }
    if layout.capacity > 0 && layout.paged && layout.end < pinned.len() {
        render_page_button(surf, fonts, state, layout.next_x(), ">");
    }
    if layout.paged {
        let x = layout.more_x();
        let bg = if state.sub_taskbar_open {
            theme::BTN_ACTIVE
        } else if is_inside(state.mouse_x, state.mouse_y, x, BUTTON_Y, MORE_WIDTH, BUTTON_HEIGHT) {
            theme::BTN_HOVER
        } else {
            theme::BTN_NORMAL
        };
        draw_button(
            surf, fonts, x, BUTTON_Y, MORE_WIDTH, BUTTON_HEIGHT, "More", bg,
            theme::TASKBAR_BORDER, theme::ACCENT_CYAN,
        );
    }

    let mut right_x = layout.tray_x;
    fill_rect(surf, right_x - 6, 10, 1, 28, theme::TASKBAR_BORDER);
    if sw >= 600 {
        for (label, online, online_color) in [
            ("NET", state.net_online, theme::ACCENT_GREEN),
            ("VFS", state.vfs_online, theme::ACCENT_GREEN),
            ("AI", state.ai_online, theme::ACCENT_CYAN),
        ] {
            let color = if online { online_color } else { theme::TEXT_MUTED };
            draw_label(
                surf, fonts, right_x, BUTTON_Y, 42, BUTTON_HEIGHT, label,
                FontFace::UiRegular, 14.0, color, true,
            );
            right_x += 48;
        }
    }
    let clock = core::str::from_utf8(&state.clock_buf).unwrap_or("12:00");
    draw_label(
        surf, fonts, right_x, BUTTON_Y, 64, BUTTON_HEIGHT, clock,
        FontFace::UiSemibold, 14.0, theme::TEXT_PRIMARY, true,
    );
    surf.damage_all();
}

fn render_page_button(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    state: &TaskbarState,
    x: i32,
    text: &str,
) {
    let bg = if is_inside(state.mouse_x, state.mouse_y, x, BUTTON_Y, PAGE_WIDTH, BUTTON_HEIGHT) {
        theme::BTN_HOVER
    } else {
        theme::BTN_NORMAL
    };
    draw_button(
        surf, fonts, x, BUTTON_Y, PAGE_WIDTH, BUTTON_HEIGHT, text, bg,
        theme::TASKBAR_BORDER, theme::TEXT_PRIMARY,
    );
}

pub fn handle_click(
    registry: &mut AppRegistry,
    state: &mut TaskbarState,
    x: i32,
    y: i32,
    width: u32,
) -> TaskbarAction {
    if is_inside(x, y, 8, BUTTON_Y, 84, BUTTON_HEIGHT)
        || is_inside(x, y, 98, BUTTON_Y, 94, BUTTON_HEIGHT)
    {
        return TaskbarAction::OpenSpotlight;
    }
    let pinned = registry.pinned_apps();
    let layout = Layout::new(width, pinned.len(), state.page_offset);
    if layout.paged && layout.start > 0
        && is_inside(x, y, APPS_X, BUTTON_Y, PAGE_WIDTH, BUTTON_HEIGHT)
    {
        state.page_offset = layout.start.saturating_sub(layout.capacity);
        return TaskbarAction::None;
    }
    let mut cursor_x = layout.apps_x();
    for app in &pinned[layout.start..layout.end] {
        if is_inside(x, y, cursor_x, BUTTON_Y, APP_WIDTH, BUTTON_HEIGHT) {
            return TaskbarAction::LaunchApp(app.path);
        }
        cursor_x += APP_STEP;
    }
    if layout.capacity > 0 && layout.paged && layout.end < pinned.len()
        && is_inside(x, y, layout.next_x(), BUTTON_Y, PAGE_WIDTH, BUTTON_HEIGHT)
    {
        state.page_offset = layout.end;
        return TaskbarAction::None;
    }
    if layout.paged
        && is_inside(x, y, layout.more_x(), BUTTON_Y, MORE_WIDTH, BUTTON_HEIGHT)
    {
        if layout.capacity == 0 {
            return TaskbarAction::OpenSpotlight;
        }
        state.sub_taskbar_open = !state.sub_taskbar_open;
        state.page_offset = if layout.end < pinned.len() { layout.end } else { 0 };
        return TaskbarAction::ToggleSubTaskbar;
    }
    TaskbarAction::None
}

fn is_inside(mx: i32, my: i32, x: i32, y: i32, w: u32, h: u32) -> bool {
    mx >= x && mx < x + w as i32 && my >= y && my < y + h as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_taskbar_more_opens_application_search() {
        let mut registry = AppRegistry::new();
        let mut state = TaskbarState::new();
        for width in [600, 640, 700, 727] {
            let layout = Layout::new(width, registry.pinned_apps().len(), 0);
            assert_eq!(layout.capacity, 0);
            assert!(layout.paged);
            assert!(layout.more_x() >= APPS_X);
            assert!(layout.more_x() + (MORE_WIDTH as i32) < layout.tray_x);
            assert!(matches!(
                handle_click(&mut registry, &mut state, layout.more_x() + 1, BUTTON_Y + 1, width),
                TaskbarAction::OpenSpotlight
            ));
        }
    }
}
