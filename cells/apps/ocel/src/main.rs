// SPDX-License-Identifier: MIT
//! Ocel — Universal Document & Web Viewer for CellOS.
//!
//! Native viewer for an HTML subset, Markdown, source code, text, PNG/JPEG/BMP and service-rendered PDF.

#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

mod doc;
mod draw;
mod image;
mod js;
mod lease;
mod loader;
mod net;
mod parser;
mod pdf;
mod resources;

use doc::Document;
use draw::{clear, fill_rect, stroke_rect, theme, Color};
use loader::load_document;
use parser::DocFormat;

use api::display::PixelFormat;
use ostd::display::ViSurface;
use ostd::input::{InputEvent, KeyState, KeySym, Modifiers, MouseButton};
use ostd::syscall::{sys_get_resolution, sys_get_time, sys_yield};
use ostd::typography::{FontFace, TextFonts};

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
    IpcSubmit,
    IpcTake,
    IpcWait,
    IpcCancel,
    GpuGetResolution,
    GetTime,
    OpenCap,
    ReadCap,
    CloseCap,
    StatCap,
    SeekCap,
    // `ostd::args()` reads the spawner's command line out of the state stash
    // (`sys_spawn_args` -> `StateRestore`), which is how `ocel <url>` gets its
    // document without the address bar.
    StateRestore
];

#[cfg(not(test))]
ostd::cell_main!(cell_main);

// Decoding must not use ostd's 1 MiB default arena.
#[cfg(target_os = "none")]
ostd::declare_custom_heap!(16 * 1024 * 1024);

const TOOLBAR_HEIGHT: u32 = 44;
const TABBAR_HEIGHT: u32 = 28;
const STATUS_HEIGHT: u32 = 24;

fn ui_text(fonts:&mut TextFonts,surf:&mut ViSurface,rect:(i32,i32,u32,u32),
    value:&str,color:Color,face:FontFace,px:f32,center:bool) {
    let (x,y,w,h)=rect;
    let left=if center {x as f32+((w as f32-fonts.measure(face,value,px))*0.5).max(0.0)}else{x as f32};
    let top=y as f32+((h as f32-fonts.line_height(face,px))*0.5).max(0.0);
    draw::text(fonts,surf,left,top,value,face,px,color,(x,y,x+w as i32,y+h as i32));
}

struct Tab {
    pub title: String,
    pub url: String,
    pub doc: Document,
    pub scroll_y: i32,
    pub history: Vec<String>,
    pub history_idx: usize,
    pub js_context_id: u64,
}

impl Tab {
    fn new(url: &str, viewport_w: u32, viewport_h: u32, js_runtime: &mut js::Tier2JsBridge, fonts:&mut TextFonts) -> Self {
        let mut tab = Self {
            title: String::new(),
            url: String::new(),
            doc: Document::new(),
            scroll_y: 0,
            history: Vec::new(),
            history_idx: 0,
            js_context_id: 0,
        };
        let (loaded_url, _fmt) = tab.load(url, viewport_w, viewport_h, js_runtime, fonts);
        tab.history = alloc::vec![loaded_url];
        tab
    }

    /// Parse `url` into this tab, run any script it carries, and lay it out.
    ///
    /// The first tab (`Tab::new`) and every later navigation both come through
    /// here, so the load-time lines (`loaded`, `dom title`, `js backend`) are
    /// emitted once per load and cannot drift between the two callers.
    ///
    /// Returns the canonical URL and the format name for the status line.
    fn load(
        &mut self,
        url: &str,
        viewport_w: u32,
        viewport_h: u32,
        js_runtime: &mut js::Tier2JsBridge,
        fonts: &mut TextFonts,
    ) -> (String, &'static str) {
        // Navigation relinquishes the old context before loading another engine.
        js_runtime.reset();
        let loaded = load_document(url);

        let (nodes, scripts, mut arena, fmt_name) = if let Some(direct) = loaded.direct_nodes {
            let format_name = if pdf::is_pdf_path(&loaded.url) { "PDF" } else { "Image" };
            (direct, alloc::vec::Vec::new(), None, format_name)
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
                    DocFormat::Source(_) => "Source",
                },
            )
        };

        self.doc.clear();
        self.doc.external_stylesheets.clear();
        if let Some(document) = &arena {
            for node in &document.nodes {
                if node.tag() == Some("link") && node.get_attribute("rel").is_some_and(|rel| rel.split_ascii_whitespace().any(|value| value.eq_ignore_ascii_case("stylesheet"))) {
                    if let Some(href) = node.get_attribute("href") {
                        match resources::load_text(&loaded.url, href) {
                            Ok(css) => self.doc.external_stylesheets.push((node.id, css)),
                            Err(error) => ostd::io::println(&format!("[ocel] stylesheet error: {}", error)),
                        }
                    }
                }
            }
        }
        let mut content_title = loaded.title;
        let scripts_ready = match js_runtime.sync_script_document(arena.as_ref(), !scripts.is_empty(), &content_title) {
            Ok(ready) => ready,
            Err(error) => {
                ostd::io::println(&format!("[ocel] DOM sync failed: {:?}", error));
                if let Some(document) = &mut arena {
                    let warning = document.alloc_node(dom_arena::NodeData::Element {
                        tag: String::from("p"), attributes: Vec::new(),
                    });
                    let text = document.alloc_node(dom_arena::NodeData::Text(format!("JavaScript unavailable: {}", error.message)));
                    document.append_child(warning, text);
                    document.append_child(document.root, warning);
                }
                false
            }
        };
        self.js_context_id = if scripts_ready { js_runtime.context_id() } else { 0 };
        use js::JsContext;
        for script in &scripts {
            if !scripts_ready { break; }
            let source = match script {
                parser::html::ScriptSource::Inline(source) => Ok(source.clone()),
                parser::html::ScriptSource::External(url) => resources::load_text(&loaded.url, url),
            };
            match source {
                Ok(source) => if let Err(error) = js_runtime.eval(&source) {
                    ostd::io::println(&format!("[ocel] script error: {:?}", error));
                },
                Err(error) => ostd::io::println(&format!("[ocel] script load error: {}", error)),
            }
            for mutation in js_runtime.take_mutations() {
                match mutation {
                    dom_arena::DomMutation::SetDocumentTitle { title } => {
                        if !title.is_empty() {
                            ostd::io::println(&format!("[ocel] dom title: {}", title));
                            content_title = title;
                        }
                    }
                    other => if let Some(document) = &mut arena { document.apply_mutation(&other); },
                }
            }
        }
        self.url = loaded.url;
        self.doc.arena = arena;
        self.title = content_title;
        self.scroll_y = 0;
        self.doc.nodes = nodes;
        if self.doc.arena.is_some() {
            self.doc.relayout_from_arena(fonts, viewport_w, viewport_h);
        } else {
            self.doc.compute_layout(fonts, viewport_w);
        }
        ostd::io::println(&format!(
            "[ocel] loaded {} ({}, {} items)",
            self.url,
            fmt_name,
            self.doc.nodes.len()
        ));
        (self.url.clone(), fmt_name)
    }
}

struct OcelViewer {
    surface: ViSurface,
    url_input: String,
    status_text: String,
    tabs: Vec<Tab>,
    active_tab: usize,
    js_runtime: js::Tier2JsBridge,
    fonts: TextFonts,
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
    fn new(comp_tid: usize, width: u32, height: u32, initial_url: &str) -> Option<Self> {
        let surface = ViSurface::create(comp_tid, width, height, PixelFormat::Bgra8888).ok()?;
        let _ = surface.set_title("Ocel Document Viewer");

        let mut js_runtime = js::Tier2JsBridge::new();
        let mut fonts = TextFonts::new();
        let viewport_w = width.saturating_sub(16);
        let first_tab = Tab::new(initial_url, viewport_w, height.saturating_sub(TOOLBAR_HEIGHT + TABBAR_HEIGHT + STATUS_HEIGHT), &mut js_runtime, &mut fonts);
        let initial_url = first_tab.url.clone();

        let viewer = Self {
            surface,
            url_input: initial_url,
            status_text: String::from("Ready - Ocel Document Viewer v0.1.0"),
            tabs: alloc::vec![first_tab],
            active_tab: 0,
            js_runtime,
            fonts,
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
        let viewport_w = self.width.saturating_sub(16);
        let tab = Tab::new(url, viewport_w, self.height.saturating_sub(TOOLBAR_HEIGHT + TABBAR_HEIGHT + STATUS_HEIGHT), &mut self.js_runtime, &mut self.fonts);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len().saturating_sub(1);
        self.url_input = self.current_tab().url.clone();
        self.status_text = format!("New tab: {}", self.url_input);
        ostd::io::println(&format!(
            "[ocel] tab {} active: {}",
            self.active_tab + 1,
            self.url_input
        ));
        if self.search_open {
            self.perform_search();
        }
    }

    fn close_tab(&mut self, idx: usize) {
        use js::JsContext;
        if self.tabs.len() > 1 && idx < self.tabs.len() {
            if self.tabs[idx].js_context_id == self.js_runtime.context_id() {
                self.js_runtime.reset();
            }
            self.tabs.remove(idx);
            if idx < self.active_tab {
                self.active_tab -= 1;
            } else if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len().saturating_sub(1);
            }
            self.url_input = self.current_tab().url.clone();
            self.status_text = format!("Closed tab. Active: {}", self.url_input);
            ostd::io::println(&format!(
                "[ocel] tab {} active: {}",
                self.active_tab + 1,
                self.url_input
            ));
            if self.search_open {
                self.perform_search();
            }
        }
    }

    fn switch_tab(&mut self, index: usize) {
        if index != self.active_tab {
            use js::JsContext;
            self.js_runtime.reset();
            self.active_tab = index;
        }
        self.url_input = self.current_tab().url.clone();
    }

    fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.switch_tab((self.active_tab + 1) % self.tabs.len());
            self.status_text = format!("Switched to tab: {}", self.url_input);
            ostd::io::println(&format!(
                "[ocel] tab {} active: {}",
                self.active_tab + 1,
                self.url_input
            ));
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
        let viewport_w = self.width.saturating_sub(16);
        let viewport_h = self.height.saturating_sub(TOOLBAR_HEIGHT + TABBAR_HEIGHT + STATUS_HEIGHT);
        let (loaded_url, fmt_name, items, height) = {
            let Self {
                tabs,
                active_tab,
                js_runtime,
                fonts,
                ..
            } = self;
            let tab = &mut tabs[*active_tab];
            let (loaded_url, fmt_name) = tab.load(url, viewport_w, viewport_h, js_runtime, fonts);
            (
                loaded_url,
                fmt_name,
                tab.doc.nodes.len(),
                tab.doc.total_height,
            )
        };
        self.url_input = loaded_url;

        if self.search_open && !self.search_query.is_empty() {
            self.perform_search();
        }

        self.status_text = format!(
            "Loaded: {} ({}, {} items, {}px)",
            self.url_input, fmt_name, items, height
        );
    }

    fn perform_search(&mut self) {
        let query = self.search_query.trim();
        if query.is_empty() {
            self.search_matches.clear();
            self.search_idx = 0;
            return;
        }

        let matches = self
            .tabs
            .get(self.active_tab)
            .map(|tab| tab.doc.search(query))
            .unwrap_or_default();

        let first_match = matches.first().copied();
        let total = matches.len();
        self.search_matches = matches;
        self.search_idx = 0;
        ostd::io::println(&format!("[ocel] search \"{}\": {} match(es)", query, total));

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
        ostd::io::println(&format!(
            "[ocel] search next: {}/{}",
            self.search_idx + 1,
            self.search_matches.len()
        ));
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
            &mut self.fonts,
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

        // Navigation chrome uses bounded, vertically centered Inter text.
        let fonts=&mut self.fonts;
        let surf=&mut self.surface;
        let regular=FontFace::UiRegular;
        let semibold=FontFace::UiSemibold;
        fill_rect(surf,0,0,w,TOOLBAR_HEIGHT,theme::BG_TOOLBAR);
        fill_rect(surf,0,TOOLBAR_HEIGHT as i32-1,w,1,theme::BORDER);
        ui_text(fonts,surf,(10,8,48,28),"Ocel",theme::ACCENT_CYAN,semibold,16.0,false);
        let tab=&self.tabs[self.active_tab];
        let can_back=tab.history_idx>0;
        let can_fwd=tab.history_idx+1<tab.history.len();
        for (x,label,enabled) in [(64,"<",can_back),(94,">",can_fwd)] {
            fill_rect(surf,x,8,26,28,if enabled {theme::ACCENT_BLUE}else{theme::BORDER});
            ui_text(fonts,surf,(x,8,26,28),label,
                if enabled {Color::rgb(17,17,27)}else{theme::TEXT_MUTED},semibold,16.0,true);
        }
        let addr_x=126;
        let addr_w=w.saturating_sub(180);
        fill_rect(surf,addr_x,8,addr_w,28,theme::BG_INPUT);
        stroke_rect(surf,addr_x,8,addr_w,28,theme::BORDER);
        ui_text(fonts,surf,(addr_x+8,8,addr_w.saturating_sub(16),28),
            &self.url_input,theme::TEXT_PRIMARY,regular,15.0,false);
        let btn_x=addr_x+addr_w as i32+8;
        fill_rect(surf,btn_x,8,38,28,theme::ACCENT_BLUE);
        ui_text(fonts,surf,(btn_x,8,38,28),"Go",Color::rgb(17,17,27),semibold,14.0,true);

        let tabbar_y=TOOLBAR_HEIGHT as i32;
        fill_rect(surf,0,tabbar_y,w,TABBAR_HEIGHT,theme::BG_TOOLBAR);
        fill_rect(surf,0,tabbar_y+TABBAR_HEIGHT as i32-1,w,1,theme::BORDER);
        let num_tabs=self.tabs.len() as u32;
        let tab_w=140u32.min(w.saturating_sub(60)/num_tabs.max(1));
        for (i,tab) in self.tabs.iter().enumerate() {
            let x=10+i as i32*tab_w as i32;
            let active=i==self.active_tab;
            fill_rect(surf,x,tabbar_y+2,tab_w.saturating_sub(2),TABBAR_HEIGHT-2,
                if active {theme::BG_DARK}else{theme::BG_INPUT});
            if active {fill_rect(surf,x,tabbar_y,tab_w.saturating_sub(2),2,theme::ACCENT_CYAN);}
            ui_text(fonts,surf,(x+6,tabbar_y+2,tab_w.saturating_sub(30),TABBAR_HEIGHT-4),
                &tab.title,if active{theme::TEXT_PRIMARY}else{theme::TEXT_MUTED},regular,14.0,false);
            if tab_w>=24 {
                ui_text(fonts,surf,(x+tab_w as i32-20,tabbar_y+2,18,TABBAR_HEIGHT-4),
                    "×",theme::TEXT_MUTED,regular,14.0,true);
            }
        }
        let plus_x=10+num_tabs as i32*tab_w as i32+6;
        if plus_x+22<w as i32 {
            fill_rect(surf,plus_x,tabbar_y+3,22,22,theme::BG_INPUT);
            ui_text(fonts,surf,(plus_x,tabbar_y+3,22,22),"+",theme::ACCENT_BLUE,regular,16.0,true);
        }

        if self.search_open {
            let sb_w=320u32.min(w.saturating_sub(30));
            let sb_x=w.saturating_sub(sb_w+30) as i32;
            let sb_y=(TOOLBAR_HEIGHT+TABBAR_HEIGHT+8) as i32;
            fill_rect(surf,sb_x,sb_y,sb_w,34,theme::BG_INPUT);
            stroke_rect(surf,sb_x,sb_y,sb_w,34,theme::ACCENT_BLUE);
            let count=if self.search_matches.is_empty(){String::from("0/0")}
                else{format!("{}/{}",self.search_idx+1,self.search_matches.len())};
            let count_w=fonts.measure(regular,&count,14.0) as u32+12;
            let label_w=fonts.measure(semibold,"Find:",14.0) as u32+8;
            ui_text(fonts,surf,(sb_x+8,sb_y,label_w,34),"Find:",theme::TEXT_MUTED,semibold,14.0,false);
            ui_text(fonts,surf,(sb_x+8+label_w as i32,sb_y,
                sb_w.saturating_sub(label_w+count_w+20),34),&self.search_query,theme::TEXT_PRIMARY,regular,14.0,false);
            ui_text(fonts,surf,(sb_x+sb_w.saturating_sub(count_w+8) as i32,sb_y,count_w,34),
                &count,theme::TEXT_MUTED,regular,14.0,true);
        }
        let status_y=h.saturating_sub(STATUS_HEIGHT) as i32;
        fill_rect(surf,0,status_y,w,STATUS_HEIGHT,theme::BG_TOOLBAR);
        fill_rect(surf,0,status_y,w,1,theme::BORDER);
        let pct=if max_s>0{active_scroll*100/max_s}else{100};
        let info=format!("Tab {}/{} · {}%",self.active_tab+1,self.tabs.len(),pct);
        let info_w=fonts.measure(regular,&info,14.0) as u32+16;
        ui_text(fonts,surf,(10,status_y+1,w.saturating_sub(info_w+30),STATUS_HEIGHT-2),
            &self.status_text,theme::TEXT_MUTED,regular,14.0,false);
        ui_text(fonts,surf,(w.saturating_sub(info_w+8) as i32,status_y+1,info_w,STATUS_HEIGHT-2),
            &info,theme::TEXT_MUTED,regular,14.0,true);

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
                        self.switch_tab(i);
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
                if active_tab.js_context_id == 0 || active_tab.js_context_id != self.js_runtime.context_id() {
                    ostd::io::println("[ocel] This tab's JavaScript context is inactive; reload to enable scripts.");
                    return;
                }
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
                if let Err(error) = self.js_runtime.dispatch_event(&event) {
                    self.status_text = format!("JavaScript: {}", error.message);
                    ostd::io::println(&format!("[ocel] {}", self.status_text));
                    self.render();
                }

                let mutations = self.js_runtime.take_mutations();
                if !mutations.is_empty() {
                    let viewport_w = self.width.saturating_sub(16);
                    let tab = &mut self.tabs[self.active_tab];
                    let mut layout_dirty = false;

                    for mutation in mutations {
                        match mutation {
                            dom_arena::DomMutation::SetDocumentTitle { title } => {
                                if !title.is_empty() {
                                    tab.title = title;
                                }
                            }
                            other => {
                                if let Some(arena) = &mut tab.doc.arena {
                                    if arena.apply_mutation(&other) {
                                        layout_dirty = true;
                                    }
                                }
                            }
                        }
                    }

                    if layout_dirty {
                        tab.doc.relayout_from_arena(&mut self.fonts, viewport_w, view_h as u32);
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
    #[cfg(target_os = "none")]
    init_custom_heap();
    ostd::io::println("[ocel] Starting Ocel Document Viewer...");

    let comp_tid = ostd::display::wait_for_compositor();

    let (mut screen_w, mut screen_h) = sys_get_resolution();
    if screen_w == 0 || screen_h == 0 || screen_w > 4096 || screen_h > 4096 {
        screen_w = 1280;
        screen_h = 800;
    }

    let win_w = screen_w.min(1024);
    let win_h = screen_h.min(680);

    // `ocel <url>` opens that document instead of the welcome page, so the
    // viewer can be used as a handler (`ocel file:///docs/readme.md`) and a test
    // can name the document it wants rendered without driving the address bar.
    let initial_url = match ostd::args().into_iter().next() {
        Some(url) if !url.is_empty() => url,
        _ => String::from("file:///welcome.md"),
    };

    let mut viewer = match OcelViewer::new(comp_tid, win_w, win_h, &initial_url) {
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
