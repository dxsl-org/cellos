// SPDX-License-Identifier: MIT
//! Bottom Taskbar renderer and interaction handler for CellOS Desktop.

use api::syscall::service;
use ostd::display::ViSurface;
use ostd::syscall::{sys_get_time, sys_lookup_service};

use crate::apps::AppRegistry;
use crate::draw::{draw_button, draw_str, fill_rect, stroke_rect, theme};

pub const TASKBAR_HEIGHT: u32 = 40;
pub const MAX_VISIBLE_APPS: usize = 4;

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

pub fn render(surf: &mut ViSurface, registry: &AppRegistry, state: &TaskbarState) {
    let sw = surf.width();
    let sh = surf.height();

    // ── 1. Bar Background & Top Border ─────────────────────────────────────────
    fill_rect(surf, 0, 0, sw, sh, theme::TASKBAR_BG);
    fill_rect(surf, 0, 0, sw, 1, theme::TASKBAR_BORDER);

    // ── 2. Left side: Logo & Spotlight Search button ───────────────────────────
    // [  CellOS ] button (x=8, y=5, w=84, h=30)
    let logo_bg = if is_inside(state.mouse_x, state.mouse_y, 8, 5, 84, 30) {
        theme::BTN_HOVER
    } else {
        theme::BTN_NORMAL
    };
    draw_button(
        surf,
        8,
        5,
        84,
        30,
        "CellOS",
        logo_bg,
        theme::TASKBAR_BORDER,
        theme::ACCENT_CYAN,
    );

    // [ 🔍 Search ] button (x=98, y=5, w=88, h=30)
    let search_bg = if is_inside(state.mouse_x, state.mouse_y, 98, 5, 88, 30) {
        theme::BTN_HOVER
    } else {
        theme::BTN_NORMAL
    };
    draw_button(
        surf,
        98,
        5,
        88,
        30,
        "? Search",
        search_bg,
        theme::ACCENT_BLUE,
        theme::TEXT_PRIMARY,
    );

    // Divider line
    fill_rect(surf, 194, 8, 1, 24, theme::TASKBAR_BORDER);

    // ── 3. Pinned Apps / Task items ────────────────────────────────────────────
    let pinned = registry.pinned_apps();
    let total_pinned = pinned.len();
    let mut cursor_x = 204i32;

    // Show previous page button if paged
    if state.page_offset > 0 {
        let prev_bg = if is_inside(state.mouse_x, state.mouse_y, cursor_x, 5, 28, 30) {
            theme::BTN_HOVER
        } else {
            theme::BTN_NORMAL
        };
        draw_button(
            surf,
            cursor_x,
            5,
            28,
            30,
            "<",
            prev_bg,
            theme::TASKBAR_BORDER,
            theme::TEXT_PRIMARY,
        );
        cursor_x += 34;
    }

    let end_idx = (state.page_offset + MAX_VISIBLE_APPS).min(total_pinned);
    for app in &pinned[state.page_offset..end_idx] {
        let btn_w = 104u32;
        let btn_h = 30u32;
        let is_hover = is_inside(state.mouse_x, state.mouse_y, cursor_x, 5, btn_w, btn_h);
        let bg = if is_hover {
            theme::BTN_HOVER
        } else {
            theme::BTN_NORMAL
        };

        fill_rect(surf, cursor_x, 5, btn_w, btn_h, bg);
        stroke_rect(surf, cursor_x, 5, btn_w, btn_h, theme::TASKBAR_BORDER);

        // Icon badge
        draw_str(surf, cursor_x + 8, 16, app.icon, theme::ACCENT_CYAN, 1);
        // App name
        draw_str(surf, cursor_x + 36, 16, app.name, theme::TEXT_PRIMARY, 1);

        cursor_x += btn_w as i32 + 6;
    }

    // Show next page button if more apps exist
    if end_idx < total_pinned {
        let next_bg = if is_inside(state.mouse_x, state.mouse_y, cursor_x, 5, 28, 30) {
            theme::BTN_HOVER
        } else {
            theme::BTN_NORMAL
        };
        draw_button(
            surf,
            cursor_x,
            5,
            28,
            30,
            ">",
            next_bg,
            theme::TASKBAR_BORDER,
            theme::TEXT_PRIMARY,
        );
        cursor_x += 34;
    }

    // Overflow button: if total pinned > MAX_VISIBLE_APPS, show More toggle
    if total_pinned > MAX_VISIBLE_APPS {
        let more_w = 64u32;
        let more_bg = if state.sub_taskbar_open {
            theme::BTN_ACTIVE
        } else if is_inside(state.mouse_x, state.mouse_y, cursor_x, 5, more_w, 30) {
            theme::BTN_HOVER
        } else {
            theme::BTN_NORMAL
        };
        draw_button(
            surf,
            cursor_x,
            5,
            more_w,
            30,
            "More",
            more_bg,
            theme::TASKBAR_BORDER,
            theme::ACCENT_CYAN,
        );
    }

    // ── 4. Right side: System Tray & Clock ─────────────────────────────────────
    let mut right_x = sw as i32 - 190;

    // Divider
    fill_rect(surf, right_x - 12, 8, 1, 24, theme::TASKBAR_BORDER);

    // Daemon status indicators: [NET] [VFS] [AI]
    let net_fg = if state.net_online {
        theme::ACCENT_GREEN
    } else {
        theme::TEXT_MUTED
    };
    draw_str(surf, right_x, 16, "NET", net_fg, 1);
    right_x += 34;

    let vfs_fg = if state.vfs_online {
        theme::ACCENT_GREEN
    } else {
        theme::TEXT_MUTED
    };
    draw_str(surf, right_x, 16, "VFS", vfs_fg, 1);
    right_x += 34;

    let ai_fg = if state.ai_online {
        theme::ACCENT_CYAN
    } else {
        theme::TEXT_MUTED
    };
    draw_str(surf, right_x, 16, "AI", ai_fg, 1);
    right_x += 34;

    // Digital Clock HH:MM
    let clock_str = core::str::from_utf8(&state.clock_buf).unwrap_or("12:00");
    draw_str(surf, right_x + 8, 16, clock_str, theme::TEXT_PRIMARY, 1);

    surf.damage_all();
}

pub fn handle_click(
    registry: &mut AppRegistry,
    state: &mut TaskbarState,
    x: i32,
    y: i32,
) -> TaskbarAction {
    if !(5..=35).contains(&y) {
        return TaskbarAction::None;
    }

    // [  CellOS ] button (x=8..92) -> open spotlight / menu
    if (8..92).contains(&x) {
        return TaskbarAction::OpenSpotlight;
    }

    // [ ? Search ] button (x=98..186) -> open spotlight
    if (98..186).contains(&x) {
        return TaskbarAction::OpenSpotlight;
    }

    let pinned = registry.pinned_apps();
    let total_pinned = pinned.len();
    let mut cursor_x = 204i32;

    // Previous page button
    if state.page_offset > 0 {
        if (cursor_x..cursor_x + 28).contains(&x) {
            state.page_offset = state.page_offset.saturating_sub(MAX_VISIBLE_APPS);
            return TaskbarAction::None;
        }
        cursor_x += 34;
    }

    let end_idx = (state.page_offset + MAX_VISIBLE_APPS).min(total_pinned);
    for app in &pinned[state.page_offset..end_idx] {
        let btn_w = 104i32;
        if (cursor_x..cursor_x + btn_w).contains(&x) {
            return TaskbarAction::LaunchApp(app.path);
        }
        cursor_x += btn_w + 6;
    }

    // Next page button
    if end_idx < total_pinned {
        if (cursor_x..cursor_x + 28).contains(&x) {
            state.page_offset += MAX_VISIBLE_APPS;
            return TaskbarAction::None;
        }
        cursor_x += 34;
    }

    // More / Overflow button
    if total_pinned > MAX_VISIBLE_APPS {
        let more_w = 64i32;
        if (cursor_x..cursor_x + more_w).contains(&x) {
            state.sub_taskbar_open = !state.sub_taskbar_open;
            if state.page_offset + MAX_VISIBLE_APPS < total_pinned {
                state.page_offset += MAX_VISIBLE_APPS;
            } else {
                state.page_offset = 0;
            }
            return TaskbarAction::ToggleSubTaskbar;
        }
    }

    TaskbarAction::None
}

fn is_inside(mx: i32, my: i32, x: i32, y: i32, w: u32, h: u32) -> bool {
    mx >= x && mx < x + w as i32 && my >= y && my < y + h as i32
}
