// SPDX-License-Identifier: MIT
//! Ocel — Universal Document & Web Viewer for CellOS.
//!
//! Native lightweight viewer for HTML/CSS, Markdown, PDF, and plain text.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

mod doc;
mod draw;
mod font;
mod image;
mod js;
mod loader;
mod net;
mod parser;

use doc::Document;
use draw::{clear, draw_str, fill_rect, stroke_rect, theme, Color};
use loader::load_document;
use parser::DocFormat;

use api::display::PixelFormat;
use ostd::display::ViSurface;
use ostd::input::{InputEvent, KeyState, KeySym, Modifiers, MouseButton};
use ostd::syscall::{sys_get_resolution, sys_get_time, sys_yield};

api::declare_manifest!(block_io = false, network = false, spawn = false);

api::declare_syscalls![
    Log,
    GrantAlloc,
    GrantRegister,
    GrantShare,
    GrantSlice,
    GrantUnregister,
    Send,
    Recv,
    TryRecv,
    LookupService,
    GpuGetResolution,
    GetTime,
    OpenCap,
    ReadCap,
    CloseCap,
    StatCap,
    SeekCap
];

ostd::cell_main!(cell_main);

const TOOLBAR_HEIGHT: u32 = 44;
const TABBAR_HEIGHT: u32 = 28;
const STATUS_HEIGHT: u32 = 24;

struct Tab {
    pub title: String,
    pub url: String,
    pub doc: Document,
    pub scroll_y: i32,
    pub history: Vec<String>,
    pub history_idx: usize,
}

impl Tab {
    fn new(url: &str, viewport_w: u32, js_runtime: &mut js::Tier2JsBridge) -> Self {
        let loaded = load_document(url);
        let mut doc = Document::new();

        let (nodes, scripts, arena, _) = if let Some(direct) = loaded.direct_nodes {
            (direct, alloc::vec::Vec::new(), None, "Image")
        } else {
            let fmt = DocFormat::detect_from_url_or_content(&loaded.url, &loaded.content);
            let (parsed_nodes, parsed_scripts, parsed_arena) =
                parser::parse_content(fmt, &loaded.content);
            (
                parsed_nodes,
                parsed_scripts,
                parsed_arena,
                match fmt {
                    DocFormat::Markdown => "Markdown",
                    DocFormat::Html => "HTML",
                    DocFormat::PlainText => "PlainText",
                },
            )
        };

        for script in &scripts {
            use js::JsContext;
            let _ = js_runtime.eval(script);
        }

        let mut content_title = loaded.title;
        use js::JsContext;
        for mutation in js_runtime.take_mutations() {
            if let dom_arena::DomMutation::SetDocumentTitle { title } = mutation {
                if !title.is_empty() {
                    content_title = title;
                }
            }
        }
        doc.arena = arena;
        doc.nodes = nodes;
        doc.compute_layout(viewport_w);

        Self {
            title: content_title,
            url: loaded.url.clone(),
            doc,
            scroll_y: 0,
            history: alloc::vec![loaded.url],
            history_idx: 0,
        }
    }
}

struct OcelViewer {
    surface: ViSurface,
    url_input: String,
    status_text: String,
    tabs: Vec<Tab>,
    active_tab: usize,
    js_runtime: js::Tier2JsBridge,
    cursor_x: i32,
    cursor_y: i32,
    // In-Document Search (Ctrl+F)
    search_open: bool,
    search_query: String,
    search_matches: Vec<i32>,
    search_idx: usize,
    width: u32,
    height: u32,
}

impl OcelViewer {
    fn new(comp_tid: usize, width: u32, height: u32) -> Option<Self> {
        let surface = ViSurface::create(comp_tid, width, height, PixelFormat::Bgra8888).ok()?;
        let _ = surface.set_title("Ocel Document Viewer");

        let mut js_runtime = js::Tier2JsBridge::new();
        let viewport_w = width.saturating_sub(24);
        let first_tab = Tab::new("file:///welcome.md", viewport_w, &mut js_runtime);
        let initial_url = first_tab.url.clone();

        let viewer = Self {
            surface,
            url_input: initial_url,
            status_text: String::from("Ready - Ocel Document Viewer v0.1.0"),
            tabs: alloc::vec![first_tab],
            active_tab: 0,
            js_runtime,
            cursor_x: 0,
            cursor_y: 0,
            search_open: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_idx: 0,
            width,
            height,
        };

        Some(viewer)
    }

    fn current_tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    fn current_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab]
    }

    fn new_tab(&mut self, url: &str) {
        let viewport_w = self.width.saturating_sub(24);
        let tab = Tab::new(url, viewport_w, &mut self.js_runtime);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len().saturating_sub(1);
        self.url_input = self.current_tab().url.clone();
        self.status_text = format!("New tab: {}", self.url_input);
        if self.search_open {
            self.perform_search();
        }
    }

    fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() > 1 && idx < self.tabs.len() {
            self.tabs.remove(idx);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len().saturating_sub(1);
            }
            self.url_input = self.current_tab().url.clone();
            self.status_text = format!("Closed tab. Active: {}", self.url_input);
            if self.search_open {
                self.perform_search();
            }
        }
    }

    fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
            self.url_input = self.current_tab().url.clone();
            self.status_text = format!("Switched to tab: {}", self.url_input);
            if self.search_open {
                self.perform_search();
            }
        }
    }

    fn navigate_to(&mut self, url: &str) {
        let tab = self.current_tab_mut();
        if tab.history_idx + 1 < tab.history.len() {
            tab.history.truncate(tab.history_idx + 1);
        }
        tab.history.push(String::from(url));
        tab.history_idx = tab.history.len().saturating_sub(1);
        self.load_url(url);
    }

    fn go_back(&mut self) {
        let tab = self.current_tab_mut();
        if tab.history_idx > 0 {
            tab.history_idx -= 1;
            let prev_url = tab.history[tab.history_idx].clone();
            self.load_url(&prev_url);
        }
    }

    fn go_forward(&mut self) {
        let tab = self.current_tab_mut();
        if tab.history_idx + 1 < tab.history.len() {
            tab.history_idx += 1;
            let next_url = tab.history[tab.history_idx].clone();
            self.load_url(&next_url);
        }
    }

    fn load_url(&mut self, url: &str) {
        let loaded = load_document(url);
        self.url_input = loaded.url.clone();

        let viewport_w = self.width.saturating_sub(24);
        let (nodes, scripts, arena, fmt_name) = if let Some(direct) = loaded.direct_nodes {
            (direct, alloc::vec::Vec::new(), None, "Image")
        } else {
            let fmt = DocFormat::detect_from_url_or_content(&self.url_input, &loaded.content);
            let (parsed_nodes, parsed_scripts, parsed_arena) =
                parser::parse_content(fmt, &loaded.content);
            (
                parsed_nodes,
                parsed_scripts,
                parsed_arena,
                match fmt {
                    DocFormat::Markdown => "Markdown",
                    DocFormat::Html => "HTML",
                    DocFormat::PlainText => "PlainText",
                },
            )
        };

        self.js_runtime.reset();
        for script in &scripts {
            use js::JsContext;
            let _ = self.js_runtime.eval(script);
        }

        let mut content_title = loaded.title;
        use js::JsContext;
        for mutation in self.js_runtime.take_mutations() {
            if let dom_arena::DomMutation::SetDocumentTitle { title } = mutation {
                if !title.is_empty() {
                    content_title = title;
                }
            }
        }

        let tab = self.current_tab_mut();
        tab.url = loaded.url;
        tab.doc.arena = arena;
        tab.title = content_title;
        tab.scroll_y = 0;
        tab.doc.clear();
        tab.doc.nodes = nodes;
        tab.doc.compute_layout(viewport_w);

        if self.search_open && !self.search_query.is_empty() {
            self.perform_search();
        }

        self.status_text = format!(
            "Loaded: {} ({}, {} items, {}px)",
            self.url_input,
            fmt_name,
            self.current_tab().doc.nodes.len(),
            self.current_tab().doc.total_height
        );
    }

    fn perform_search(&mut self) {
        let query = self.search_query.trim();
        if query.is_empty() {
            self.search_matches.clear();
            self.search_idx = 0;
            return;
        }

        let q_lower = query.to_ascii_lowercase();
        let mut matches = Vec::new();

        if let Some(tab) = self.tabs.get(self.active_tab) {
            for b in &tab.doc.layout_boxes {
                let mut matched = false;
                for line in &b.lines {
                    for span in &line.spans {
                        let s_lower = span.text.to_ascii_lowercase();
                        if s_lower.contains(&q_lower) {
                            matches.push(b.y_offset);
                            matched = true;
                            break;
                        }
                    }
                    if matched {
                        break;
                    }
                }
            }
        }

        let first_match = matches.first().copied();
        self.search_matches = matches;
        self.search_idx = 0;

        if let Some(y) = first_match {
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.scroll_y = y.saturating_sub(60).max(0);
            }
        }
    }

    fn next_search_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_idx = (self.search_idx + 1) % self.search_matches.len();
        let target_y = self.search_matches[self.search_idx];
        self.current_tab_mut().scroll_y = target_y.saturating_sub(60).max(0);
    }

    fn viewport_height(&self) -> u32 {
        self.height
            .saturating_sub(TOOLBAR_HEIGHT + TABBAR_HEIGHT + STATUS_HEIGHT)
    }

    fn max_scroll(&self) -> i32 {
        let vh = self.viewport_height() as i32;
        (self.current_tab().doc.total_height - vh).max(0)
    }

    fn scroll_by(&mut self, delta: i32) {
        let max_s = self.max_scroll();
        let tab = self.current_tab_mut();
        tab.scroll_y = (tab.scroll_y + delta).clamp(0, max_s);
    }

    fn render(&mut self) {
        let w = self.width;
        let h = self.height;
        let view_h = self.viewport_height();
        let view_y = (TOOLBAR_HEIGHT + TABBAR_HEIGHT) as i32;

        // 1. Clear background
        clear(&mut self.surface, theme::BG_DARK);

        // 2. Render Active Tab Document in Viewport
        let active_scroll = self.tabs[self.active_tab].scroll_y;
        self.tabs[self.active_tab].doc.render_viewport(
            &mut self.surface,
            0,
            view_y,
            w.saturating_sub(16),
            view_h,
            active_scroll,
        );
        // 3. Render Scrollbar on the right edge if content overflows
        let max_s = self.max_scroll();
        if max_s > 0 {
            let sb_x = (w.saturating_sub(12)) as i32;
            let track_h = view_h as i32;

            fill_rect(
                &mut self.surface,
                sb_x,
                view_y,
                12,
                view_h,
                theme::BG_TOOLBAR,
            );

            let doc_h = self.current_tab().doc.total_height;
            let thumb_h = ((view_h as f32 / doc_h as f32) * (track_h as f32)).max(20.0) as u32;
            let scroll_ratio = active_scroll as f32 / max_s as f32;
            let available_track = track_h.saturating_sub(thumb_h as i32);
            let thumb_y = view_y + (scroll_ratio * available_track as f32) as i32;

            fill_rect(
                &mut self.surface,
                sb_x + 2,
                thumb_y,
                8,
                thumb_h,
                theme::ACCENT_BLUE,
            );
        }

        // 4. Top Navigation & Toolbar (Height: 44px)
        fill_rect(
            &mut self.surface,
            0,
            0,
            w,
            TOOLBAR_HEIGHT,
            theme::BG_TOOLBAR,
        );
        stroke_rect(
            &mut self.surface,
            0,
            (TOOLBAR_HEIGHT - 1) as i32,
            w,
            1,
            theme::BORDER,
        );

        // App Logo: [Ocel]
        draw_str(&mut self.surface, 10, 14, "[Ocel]", theme::ACCENT_CYAN, 1);

        // [<] Back button (x=64..90)
        let can_back = self.current_tab().history_idx > 0;
        let back_bg = if can_back {
            theme::ACCENT_BLUE
        } else {
            theme::BORDER
        };
        fill_rect(&mut self.surface, 64, 8, 26, 28, back_bg);
        draw_str(
            &mut self.surface,
            73,
            14,
            "<",
            if can_back {
                Color::rgb(17, 17, 27)
            } else {
                theme::TEXT_MUTED
            },
            1,
        );

        // [>] Forward button (x=94..120)
        let can_fwd = self.current_tab().history_idx + 1 < self.current_tab().history.len();
        let fwd_bg = if can_fwd {
            theme::ACCENT_BLUE
        } else {
            theme::BORDER
        };
        fill_rect(&mut self.surface, 94, 8, 26, 28, fwd_bg);
        draw_str(
            &mut self.surface,
            103,
            14,
            ">",
            if can_fwd {
                Color::rgb(17, 17, 27)
            } else {
                theme::TEXT_MUTED
            },
            1,
        );

        // Address bar box (x=126 .. w - 54)
        let addr_x = 126;
        let addr_w = w.saturating_sub(180);
        fill_rect(&mut self.surface, addr_x, 8, addr_w, 28, theme::BG_INPUT);
        stroke_rect(&mut self.surface, addr_x, 8, addr_w, 28, theme::BORDER);
        draw_str(
            &mut self.surface,
            addr_x + 8,
            14,
            &self.url_input,
            theme::TEXT_PRIMARY,
            1,
        );

        // Action button [Go]
        let btn_x = addr_x + addr_w as i32 + 8;
        fill_rect(&mut self.surface, btn_x, 8, 38, 28, theme::ACCENT_BLUE);
        draw_str(
            &mut self.surface,
            btn_x + 11,
            14,
            "Go",
            Color::rgb(17, 17, 27),
            1,
        );

        // 5. Tab Bar (Height: 28px, y = 44..72)
        let tabbar_y = TOOLBAR_HEIGHT as i32;
        fill_rect(
            &mut self.surface,
            0,
            tabbar_y,
            w,
            TABBAR_HEIGHT,
            theme::BG_TOOLBAR,
        );
        stroke_rect(
            &mut self.surface,
            0,
            tabbar_y + TABBAR_HEIGHT as i32 - 1,
            w,
            1,
            theme::BORDER,
        );

        let max_tab_w = 140u32;
        let num_tabs = self.tabs.len() as u32;
        let tab_w = max_tab_w.min(w.saturating_sub(60) / num_tabs.max(1));

        for (i, tab) in self.tabs.iter().enumerate() {
            let tab_x = 10 + (i as i32) * (tab_w as i32);
            let is_active = i == self.active_tab;

            let tab_bg = if is_active {
                theme::BG_DARK
            } else {
                theme::BG_INPUT
            };
            fill_rect(
                &mut self.surface,
                tab_x,
                tabbar_y + 2,
                tab_w - 2,
                TABBAR_HEIGHT - 2,
                tab_bg,
            );

            if is_active {
                // Top accent indicator line
                fill_rect(
                    &mut self.surface,
                    tab_x,
                    tabbar_y,
                    tab_w - 2,
                    2,
                    theme::ACCENT_CYAN,
                );
            }

            // Truncate title for tab display
            let title_chars: Vec<char> = tab.title.chars().collect();
            let max_display_chars = (tab_w.saturating_sub(28) / 8) as usize;
            let display_title: String = if title_chars.len() > max_display_chars {
                title_chars[..max_display_chars.max(1)].iter().collect()
            } else {
                tab.title.clone()
            };

            let title_color = if is_active {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
            };
            draw_str(
                &mut self.surface,
                tab_x + 6,
                tabbar_y + 8,
                &display_title,
                title_color,
                1,
            );

            // [x] close tab button
            let close_x = tab_x + tab_w as i32 - 16;
            draw_str(
                &mut self.surface,
                close_x,
                tabbar_y + 8,
                "x",
                theme::TEXT_MUTED,
                1,
            );
        }

        // [+] New Tab Button
        let plus_x = 10 + (num_tabs as i32) * (tab_w as i32) + 6;
        if plus_x + 22 < w as i32 {
            fill_rect(
                &mut self.surface,
                plus_x,
                tabbar_y + 3,
                22,
                22,
                theme::BG_INPUT,
            );
            draw_str(
                &mut self.surface,
                plus_x + 7,
                tabbar_y + 7,
                "+",
                theme::ACCENT_BLUE,
                1,
            );
        }

        // 6. In-Document Search Bar (Floating at top-right if open)
        if self.search_open {
            let sb_w = 320u32;
            let sb_h = 32u32;
            let sb_x = (w.saturating_sub(sb_w + 30)) as i32;
            let sb_y = (TOOLBAR_HEIGHT + TABBAR_HEIGHT + 8) as i32;

            fill_rect(&mut self.surface, sb_x, sb_y, sb_w, sb_h, theme::BG_INPUT);
            stroke_rect(
                &mut self.surface,
                sb_x,
                sb_y,
                sb_w,
                sb_h,
                theme::ACCENT_BLUE,
            );

            let count_info = if self.search_matches.is_empty() {
                String::from("0/0")
            } else {
                format!("{}/{}", self.search_idx + 1, self.search_matches.len())
            };
            let search_text = format!("Find: {} [{}]", self.search_query, count_info);
            draw_str(
                &mut self.surface,
                sb_x + 10,
                sb_y + 8,
                &search_text,
                theme::TEXT_PRIMARY,
                1,
            );
        }

        // 7. Bottom Status Bar (Height: 24px)
        let status_y = (h.saturating_sub(STATUS_HEIGHT)) as i32;
        fill_rect(
            &mut self.surface,
            0,
            status_y,
            w,
            STATUS_HEIGHT,
            theme::BG_TOOLBAR,
        );
        stroke_rect(&mut self.surface, 0, status_y, w, 1, theme::BORDER);

        let scroll_pct = if max_s > 0 {
            (active_scroll * 100) / max_s
        } else {
            100
        };
        let status_line = format!(
            "{} | Tab {}/{} | Scroll: {}%",
            self.status_text,
            self.active_tab + 1,
            self.tabs.len(),
            scroll_pct
        );
        draw_str(
            &mut self.surface,
            10,
            status_y + 6,
            &status_line,
            theme::TEXT_MUTED,
            1,
        );

        // Flush update to Compositor
        self.surface.damage_all();
    }

    fn handle_mouse_move(&mut self, x: i32, y: i32) {
        self.cursor_x = x;
        self.cursor_y = y;
    }

    fn handle_mouse_scroll(&mut self, dy: i32) {
        self.scroll_by(-dy * 32);
        self.render();
    }

    fn handle_mouse_button(&mut self, button: MouseButton, state: KeyState) {
        if button != MouseButton::Left || state != KeyState::Pressed {
            return;
        }

        let cx = self.cursor_x;
        let cy = self.cursor_y;
        let w = self.width;
        let view_y = (TOOLBAR_HEIGHT + TABBAR_HEIGHT) as i32;
        let view_h = self.viewport_height() as i32;

        // 1. Toolbar Back button [<]
        if (64..90).contains(&cx) && (8..36).contains(&cy) {
            self.go_back();
            self.render();
            return;
        }

        // 2. Toolbar Forward button [>]
        if (94..120).contains(&cx) && (8..36).contains(&cy) {
            self.go_forward();
            self.render();
            return;
        }

        // 3. Toolbar [Go] button
        let addr_x = 126;
        let addr_w = w.saturating_sub(180) as i32;
        let btn_x = addr_x + addr_w + 8;
        if (btn_x..btn_x + 38).contains(&cx) && (8..36).contains(&cy) {
            let target = self.url_input.clone();
            self.navigate_to(&target);
            self.render();
            return;
        }

        // 4. Tab Bar clicks (y in 44..72)
        let tabbar_y = TOOLBAR_HEIGHT as i32;
        if (tabbar_y..tabbar_y + TABBAR_HEIGHT as i32).contains(&cy) {
            let max_tab_w = 140u32;
            let num_tabs = self.tabs.len() as u32;
            let tab_w = max_tab_w.min(w.saturating_sub(60) / num_tabs.max(1));

            // Check click on tabs
            for i in 0..self.tabs.len() {
                let tab_x = 10 + (i as i32) * (tab_w as i32);
                let tab_x_end = tab_x + tab_w as i32;

                if (tab_x..tab_x_end).contains(&cx) {
                    let close_btn_x = tab_x_end - 20;
                    if cx >= close_btn_x {
                        // Clicked close [x]
                        self.close_tab(i);
                    } else {
                        // Clicked tab body -> switch tab
                        self.active_tab = i;
                        self.url_input = self.current_tab().url.clone();
                    }
                    self.render();
                    return;
                }
            }

            // Check click on [+] new tab button
            let plus_x = 10 + (num_tabs as i32) * (tab_w as i32) + 6;
            if (plus_x..plus_x + 22).contains(&cx) {
                self.new_tab("file:///welcome.md");
                self.render();
                return;
            }
        }

        // 5. Scrollbar click
        let max_s = self.max_scroll();
        if max_s > 0 {
            let sb_x = (w.saturating_sub(12)) as i32;
            if cx >= sb_x && cx < sb_x + 12 && cy >= view_y && cy < view_y + view_h {
                let ratio = (cy - view_y) as f32 / view_h as f32;
                self.current_tab_mut().scroll_y = ((ratio * max_s as f32) as i32).clamp(0, max_s);
                self.render();
                return;
            }
        }

        // 6. Viewport link and interactive node click
        if cy >= view_y && cy < view_h + view_y && cx >= 0 && cx < (w.saturating_sub(16)) as i32 {
            let active_tab = &self.tabs[self.active_tab];
            let active_scroll = active_tab.scroll_y;

            // Check Hyperlink navigation
            if let Some(link_url) = active_tab
                .doc
                .hit_test_link(cx, cy, 0, view_y, active_scroll)
            {
                ostd::io::print("[ocel] Clicked link -> navigating to: ");
                ostd::io::println(&link_url);
                self.navigate_to(&link_url);
                self.render();
                return;
            }

            // Check Interactive DOM Node click -> dispatch Click event to Tier 2 JS
            if let Some(node_id) = active_tab
                .doc
                .hit_test_node(cx, cy, 0, view_y, active_scroll)
            {
                ostd::io::print("[ocel] Clicked DOM Node #");
                ostd::io::print_usize(node_id.index());
                ostd::io::println(" -> dispatching Click event to Tier 2 JS");

                let event = dom_arena::DomEvent {
                    target: node_id,
                    kind: dom_arena::EventKind::Click,
                    client_x: cx,
                    client_y: cy,
                    key: None,
                };

                use js::JsContext;
                let _ = self.js_runtime.dispatch_event(&event);

                let mutations = self.js_runtime.take_mutations();
                if !mutations.is_empty() {
                    let viewport_w = self.width.saturating_sub(24);
                    let tab = self.current_tab_mut();
                    let mut layout_dirty = false;

                    for mutation in mutations {
                        match mutation {
                            dom_arena::DomMutation::SetDocumentTitle { title } => {
                                if !title.is_empty() {
                                    tab.title = title;
                                }
                            }
                            other => {
                                if let Some(ref mut arena) = tab.doc.arena {
                                    if arena.apply_mutation(&other) {
                                        layout_dirty = true;
                                    }
                                }
                            }
                        }
                    }

                    if layout_dirty {
                        if let Some(ref arena) = tab.doc.arena {
                            tab.doc.nodes = parser::html::arena_to_doc_nodes(arena);
                            tab.doc.compute_layout(viewport_w);
                        }
                    }
                    self.render();
                }
            }
        }
    }

    fn handle_key(&mut self, sym: KeySym, ch: Option<char>, modifiers: Modifiers) {
        // Global Ctrl+T: New Tab
        if ch == Some('t') && modifiers.contains(Modifiers::CTRL) {
            self.new_tab("file:///welcome.md");
            self.render();
            return;
        }

        // Global Ctrl+W: Close Tab
        if ch == Some('w') && modifiers.contains(Modifiers::CTRL) {
            self.close_tab(self.active_tab);
            self.render();
            return;
        }

        // Global Ctrl+Tab: Next Tab
        if sym == KeySym::Tab && modifiers.contains(Modifiers::CTRL) {
            self.next_tab();
            self.render();
            return;
        }

        // Global Ctrl+F / F3: Toggle search
        if sym == KeySym::F3 || (ch == Some('f') && modifiers.contains(Modifiers::CTRL)) {
            self.search_open = !self.search_open;
            if self.search_open {
                self.perform_search();
            }
            self.render();
            return;
        }

        // Escape: Close search bar
        if sym == KeySym::Escape && self.search_open {
            self.search_open = false;
            self.render();
            return;
        }

        // In search mode
        if self.search_open {
            match sym {
                KeySym::Return => {
                    self.next_search_match();
                    self.render();
                }
                KeySym::Backspace => {
                    self.search_query.pop();
                    self.perform_search();
                    self.render();
                }
                _ => {
                    if let Some(c) = ch {
                        if !c.is_control() {
                            self.search_query.push(c);
                            self.perform_search();
                            self.render();
                        }
                    }
                }
            }
            return;
        }

        // Standard Document Navigation
        match sym {
            KeySym::Return => {
                let target = self.url_input.clone();
                self.navigate_to(&target);
                self.render();
            }
            KeySym::Backspace => {
                self.url_input.pop();
                self.render();
            }
            KeySym::Up => {
                self.scroll_by(-24);
                self.render();
            }
            KeySym::Down => {
                self.scroll_by(24);
                self.render();
            }
            KeySym::PageUp => {
                let step = (self.viewport_height() as i32).saturating_sub(48);
                self.scroll_by(-step);
                self.render();
            }
            KeySym::PageDown => {
                let step = (self.viewport_height() as i32).saturating_sub(48);
                self.scroll_by(step);
                self.render();
            }
            KeySym::Home => {
                self.current_tab_mut().scroll_y = 0;
                self.render();
            }
            KeySym::End => {
                let max_s = self.max_scroll();
                self.current_tab_mut().scroll_y = max_s;
                self.render();
            }
            _ => {
                if let Some(c) = ch {
                    if !c.is_control() {
                        self.url_input.push(c);
                        self.render();
                    }
                }
            }
        }
    }
}

fn cell_main() {
    ostd::io::println("[ocel] Starting Ocel Document Viewer...");

    let comp_tid = ostd::display::wait_for_compositor();

    let (mut screen_w, mut screen_h) = sys_get_resolution();
    if screen_w == 0 || screen_h == 0 || screen_w > 4096 || screen_h > 4096 {
        screen_w = 1280;
        screen_h = 800;
    }

    let win_w = screen_w.min(1024);
    let win_h = screen_h.min(680);

    let mut viewer = match OcelViewer::new(comp_tid, win_w, win_h) {
        Some(v) => v,
        None => {
            ostd::io::println("[ocel] Failed to create ViSurface. Exiting.");
            return;
        }
    };

    let win_x = (screen_w as i32 - win_w as i32) / 2;
    let win_y = (screen_h as i32 - win_h as i32) / 2;
    viewer.surface.move_to(win_x.max(0), win_y.max(0));

    while !ostd::input::request_focus() {
        sys_yield();
    }

    viewer.render();
    ostd::io::println("[ocel] Window initialized and painted.");

    let mut last_heartbeat = sys_get_time();
    loop {
        for event in ostd::input::poll_events(16) {
            match event {
                InputEvent::Key(ke) => {
                    if ke.state == KeyState::Pressed {
                        viewer.handle_key(ke.keysym, ke.char(), ke.modifiers);
                    }
                }
                InputEvent::MouseMove { x, y, .. } => {
                    viewer.handle_mouse_move(x, y);
                }
                InputEvent::MouseButton { button, state } => {
                    viewer.handle_mouse_button(button, state);
                }
                InputEvent::MouseScroll { dy, .. } => {
                    viewer.handle_mouse_scroll(dy);
                }
            }
        }

        let now = sys_get_time();
        if now.saturating_sub(last_heartbeat) > 5_000_000 {
            last_heartbeat = now;
        }

        sys_yield();
    }
}
