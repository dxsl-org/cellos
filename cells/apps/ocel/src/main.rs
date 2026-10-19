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

mod budget;
mod browser;
mod browser_runtime;
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

use doc::{DocNode, Document};
use draw::{fill_rect, stroke_rect, theme, Color};
use loader::{load_document, LoadedDocument, DocumentLoadJob};
use parser::DocFormat;

use api::display::PixelFormat;
use ostd::display::ViSurface;
use ostd::input::{InputEvent, KeyState, KeySym, Modifiers, MouseButton};
use ostd::syscall::{sys_get_resolution, sys_yield};
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
ostd::declare_custom_heap!(budget::HEAP_BYTES);

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
    pub scripts_enabled: bool,
    pub resources: resources::ResourceBundle,
    native_dom_dirty: bool,
    browser: browser_runtime::DocumentBrowser,
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
            scripts_enabled: true,
            resources: resources::ResourceBundle::default(),
            native_dom_dirty: false,
            browser: browser_runtime::DocumentBrowser::new(url),
        };
        let (loaded_url, _fmt) = tab.install_loaded(load_document(url), viewport_w, viewport_h, js_runtime, fonts);
        tab.history = alloc::vec![loaded_url];
        tab
    }

    fn resident_bytes(&self) -> usize {
        self.doc.resident_bytes().saturating_add(self.resources.resident_bytes())
            .saturating_add(self.title.capacity()).saturating_add(self.url.capacity())
            .saturating_add(self.browser.resident_bytes())
            .saturating_add(self.history.capacity() * core::mem::size_of::<String>())
            .saturating_add(self.history.iter().map(String::capacity).sum::<usize>())
    }

    /// Install a completed load; navigation never waits for network IPC here.
    fn install_loaded(
        &mut self,
        mut loaded: LoadedDocument,
        viewport_w: u32,
        viewport_h: u32,
        js_runtime: &mut js::Tier2JsBridge,
        fonts: &mut TextFonts,
    ) -> (String, &'static str) {
        // Navigation relinquishes the old context before loading another engine.
        js_runtime.reset();
        let previous_resources = resources::replace(core::mem::take(&mut loaded.resources));
        let mut load_error = None;

        let (nodes, scripts, mut arena, fmt_name) = if let Some(direct) = loaded.direct_nodes {
            let format_name = if pdf::is_pdf_path(&loaded.url) { "PDF" } else { "Image" };
            (direct, alloc::vec::Vec::new(), None, format_name)
        } else {
            let fmt = DocFormat::detect_from_url_or_content(&loaded.url, &loaded.content);
            let (parsed_nodes, parsed_scripts, parsed_arena) =
                match parser::parse_content_with_base_checked(fmt, &loaded.content, Some(&loaded.url)) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        load_error = Some(error);
                        (Vec::new(), Vec::new(), None)
                    }
                };
            (
                parsed_nodes,
                parsed_scripts,
                parsed_arena,
                match fmt {
                    DocFormat::Markdown => "Markdown",
                    DocFormat::Html => "HTML",
                    DocFormat::PlainText => "PlainText",
                    DocFormat::Source(_) => "Source",
                    DocFormat::Feed => "Feed",
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
        let cookies = self.browser.session.cookies();
        self.browser = browser_runtime::DocumentBrowser::new(&loaded.url);
        self.browser.session.set_cookies(cookies);
        self.browser.session.accept_response_cookies(&loaded.url, &loaded.response_headers);
        let mut scripts_ready = match js_runtime.sync_script_document(arena.as_ref(), self.scripts_enabled && !scripts.is_empty(), &loaded.title) {
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
        self.url = loaded.url.clone();
        self.title = loaded.title;
        self.doc.arena = arena.take();
        self.doc.nodes = nodes;
        if self.doc.arena.is_some() {
            self.doc.relayout_from_arena(fonts, viewport_w, viewport_h);
        } else {
            self.doc.compute_layout(fonts, viewport_w);
        }
        use js::JsContext;
        if scripts_ready {
            let configured = self.configure_browser(js_runtime, viewport_w, viewport_h)
                .and_then(|_| self.pump_browser_frame(js_runtime, fonts, viewport_w, viewport_h, false));
            if let Err(error) = configured {
                ostd::io::println(&format!("[ocel] browser initialization failed: {error}"));
                js_runtime.reset();
                scripts_ready = false;
            }
        }
        self.js_context_id = if scripts_ready { js_runtime.context_id() } else { 0 };
        for script in &scripts {
            if !scripts_ready { break; }
            let (node, source) = match script {
                parser::html::ScriptSource::Inline(node) => (*node, self.doc.arena.as_ref()
                    .map(|arena| arena.get_text_content(*node)).ok_or_else(|| String::from("Script document unavailable"))),
                parser::html::ScriptSource::External { node, url } => (*node, resources::load_text(&loaded.url, url)),
            };
            match source {
                Ok(source) => if let Err(error) = self.eval_browser_script(js_runtime, &source, node, fonts, viewport_w, viewport_h) {
                    ostd::io::println(&format!("[ocel] script error: {:?}", error));
                },
                Err(error) => ostd::io::println(&format!("[ocel] script load error: {}", error)),
            }
            if let Err(error) = self.drain_browser_output(js_runtime, fonts, viewport_w, viewport_h) {
                ostd::io::println(&format!("[ocel] JavaScript mutations rejected: {error}"));
                js_runtime.reset();
                self.browser.cancel();
                self.js_context_id = 0;
                scripts_ready = false;
            }
        }
        if scripts_ready {
            if let Err(error) = self.pump_browser_frame(js_runtime, fonts, viewport_w, viewport_h, true) {
                ostd::io::println(&format!("[ocel] browser ready event: {error}"));
            }
            if let Err(error) = self.drain_browser_output(js_runtime, fonts, viewport_w, viewport_h) {
                ostd::io::println(&format!("[ocel] JavaScript mutations rejected: {error}"));
                js_runtime.reset(); self.browser.cancel(); self.js_context_id = 0;
            }
        }
        self.url = loaded.url;
        self.scroll_y = 0;
        if load_error.is_some() { self.doc.last_error = load_error; }
        self.resources = resources::replace(previous_resources);
        if let Some((_, fragment)) = self.url.split_once('#') {
            if let Some(y) = self.doc.anchor_y(fragment) { self.scroll_y = y.max(0); }
        }
        (self.url.clone(), fmt_name)
    }
}

#[derive(Clone, Copy)]
enum Navigation {
    Push,
    Traverse(usize),
    Replace,
}

struct PendingLoad {
    job: DocumentLoadJob,
    navigation: Navigation,
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
    focused_input: Option<dom_arena::NodeId>,
    pending_load: Option<PendingLoad>,
    toc_open: bool,
    toc_entries: Vec<doc::TocEntry>,
    toc_offset: usize,
    reader_theme: draw::ReaderThemeMode,
    zoomed_image: Option<(u32, u32, alloc::rc::Rc<Vec<u8>>)>,
    pub should_exit: bool,
}

impl OcelViewer {
    fn new(comp_tid: usize, width: u32, height: u32, initial_url: &str) -> Option<Self> {
        let surface = ViSurface::create(comp_tid, width, height, PixelFormat::Bgra8888).ok()?;
        let _ = surface.set_title("Ocel Document Viewer");

        let mut js_runtime = js::Tier2JsBridge::new();
        let mut fonts = TextFonts::new();
        let viewport_w = width.saturating_sub(16);
        let first_tab = Tab::new("file:///welcome.md", viewport_w, height.saturating_sub(TOOLBAR_HEIGHT + TABBAR_HEIGHT + STATUS_HEIGHT), &mut js_runtime, &mut fonts);
        let requested_url = String::from(initial_url);
        let initial_url = first_tab.url.clone();

        let mut viewer = Self {
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
            focused_input: None,
            pending_load: None,
            toc_open: false,
            toc_entries: Vec::new(),
            toc_offset: 0,
            reader_theme: draw::ReaderThemeMode::Dark,
            zoomed_image: None,
            should_exit: false,
        };
        if requested_url != viewer.url_input { viewer.load_url(&requested_url); }

        Some(viewer)
    }

    fn current_tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    fn current_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab]
    }

    fn new_tab(&mut self, url: &str) {
        if self.tabs.len() >= budget::MAX_TABS {
            self.status_text = format!("Tab limit reached ({}). Close a tab first.", budget::MAX_TABS);
            return;
        }
        self.current_tab_mut().browser.cancel();
        self.cancel_loading();
        if self.tabs.iter().map(Tab::resident_bytes).sum::<usize>().saturating_add(128 * 1024) > budget::MAX_RESIDENT_BYTES {
            self.status_text = String::from("Close a tab to free memory before opening another.");
            return;
        }
        self.clear_transient_state();
        let viewport_w = self.width.saturating_sub(16);
        let tab = Tab::new("file:///welcome.md", viewport_w, self.height.saturating_sub(TOOLBAR_HEIGHT + TABBAR_HEIGHT + STATUS_HEIGHT), &mut self.js_runtime, &mut self.fonts);
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
        if url != "file:///welcome.md" { self.load_url(url); }
    }

    fn close_tab(&mut self, idx: usize) {
        use js::JsContext;
        if self.tabs.len() <= 1 {
            self.should_exit = true;
            return;
        }
        if idx < self.tabs.len() {
            self.clear_transient_state();
            if self.tabs[idx].js_context_id == self.js_runtime.context_id() {
                self.tabs[idx].browser.cancel();
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
        if index >= self.tabs.len() { return; }
        if index != self.active_tab {
            self.cancel_loading();
            self.clear_transient_state();
            self.current_tab_mut().browser.cancel();
            use js::JsContext;
            self.js_runtime.reset();
            self.active_tab = index;
        }
        self.url_input = self.current_tab().url.clone();
        let w = self.width.saturating_sub(16);
        let h = self.viewport_height();
        let tab = &mut self.tabs[self.active_tab];
        if tab.doc.layout_boxes.is_empty() && tab.doc.web_layout.is_none() {
            if tab.doc.is_reader_mode() {
                tab.doc.compute_layout(&mut self.fonts, w);
            } else if tab.doc.arena.is_some() {
                tab.doc.relayout_from_arena(&mut self.fonts, w, h);
            }
        }
        if self.search_open { self.perform_search(); }
    }

    fn hibernate_inactive_tabs(&mut self) {
        let active = self.active_tab;
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if i != active {
                tab.doc.web_layout = None;
                tab.doc.layout_boxes.clear();
            }
        }
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
        let resolved = if url.starts_with('#') {
            format!("{}{}", self.current_tab().url.split('#').next().unwrap_or(""), url)
        } else if !url.contains("://") {
            let (reference, fragment) = url.split_once('#').map_or((url,None), |(path,fragment)| (path,Some(fragment)));
            let mut result = resources::resolve(&self.current_tab().url, reference).unwrap_or_else(|_| String::from(reference));
            if let Some(fragment) = fragment { result.push('#'); result.push_str(fragment); }
            result
        } else { String::from(url) };
        let current_base = self.current_tab().url.split('#').next().unwrap_or("");
        if let Some((base, fragment)) = resolved.split_once('#') {
            if base == current_base {
                if let Some(y) = self.current_tab().doc.anchor_y(fragment) {
                    self.cancel_loading();
                    let tab = self.current_tab_mut();
                    tab.url = resolved.clone();
                    tab.scroll_y = y.max(0);
                    tab.history.truncate(tab.history_idx + 1);
                    if tab.history.len() == 128 { tab.history.remove(0); }
                    tab.history.push(resolved.clone());
                    tab.history_idx = tab.history.len() - 1;
                    self.url_input = resolved;
                    return;
                }
            }
        }
        self.start_load(&resolved, Navigation::Push);
    }

    fn go_back(&mut self) {
        let index = self.current_tab().history_idx;
        if index > 0 {
            let url = self.current_tab().history[index - 1].clone();
            self.start_load(&url, Navigation::Traverse(index - 1));
        }
    }

    fn go_forward(&mut self) {
        let index = self.current_tab().history_idx + 1;
        if let Some(url) = self.current_tab().history.get(index).cloned() {
            self.start_load(&url, Navigation::Traverse(index));
        }
    }

    fn clear_transient_state(&mut self) {
        self.clear_focus_and_contents();
        self.search_matches.clear();
        self.search_idx = 0;
    }

    fn clear_focus_and_contents(&mut self) {
        self.focused_input = None;
        self.current_tab_mut().doc.focus_input(None);
        self.toc_open = false;
        self.toc_entries.clear();
        self.toc_offset = 0;
    }

    fn cancel_loading(&mut self) {
        if let Some(mut pending) = self.pending_load.take() {
            pending.job.cancel();
            self.status_text = String::from("Loading cancelled");
            self.url_input = self.current_tab().url.clone();
        }
    }

    fn load_url(&mut self, url: &str) {
        self.start_load(url, Navigation::Replace);
    }

    fn start_load(&mut self, url: &str, navigation: Navigation) {
        self.cancel_loading();
        self.clear_focus_and_contents();
        let used = self.tabs.iter().map(Tab::resident_bytes).sum::<usize>();
        let remaining = budget::MAX_RESIDENT_BYTES.saturating_sub(used);
        if remaining < 128 * 1024 {
            self.status_text = String::from("Close a tab to free loading memory.");
            return;
        }
        match DocumentLoadJob::new(url) {
            Ok(mut job) => {
                job.set_memory_budget(remaining);
                let was_explicitly_web = self.tabs[self.active_tab].doc.arena.is_some() && !self.tabs[self.active_tab].doc.is_reader_mode();
                let enable_reader = !was_explicitly_web;
                if enable_reader {
                    job.set_reader_mode(true);
                }
                self.url_input = String::from(url);
                self.status_text = format!("Loading {} — Esc cancels", url);
                self.pending_load = Some(PendingLoad { job, navigation });
            }
            Err(error) => self.status_text = format!("Cannot load: {}", error),
        }
    }

    fn poll_loading(&mut self) {
        let Some(mut pending) = self.pending_load.take() else { return; };
        if self.tabs.iter().map(Tab::resident_bytes).sum::<usize>()
            .saturating_add(pending.job.resident_bytes()) > budget::MAX_RESIDENT_BYTES {
            self.hibernate_inactive_tabs();
        }
        if self.tabs.iter().map(Tab::resident_bytes).sum::<usize>()
            .saturating_add(pending.job.resident_bytes()) > budget::MAX_RESIDENT_BYTES {
            pending.job.cancel();
            self.status_text = String::from("Loading exceeded the shared tab memory budget.");
            self.render_chrome();
            return;
        }

        // Instant Text Streaming: stream early text document as soon as HTML payload arrives
        if let Some(early) = pending.job.take_early_document() {
            self.finish_load(early, pending.navigation);
            self.status_text = format!("{} — Esc cancels", pending.job.progress());
            self.render();
        }

        match pending.job.poll() {
            core::task::Poll::Pending => {
                let status = format!("{} — Esc cancels", pending.job.progress());
                let changed = status != self.status_text;
                self.status_text = status;
                self.pending_load = Some(pending);
                if changed { self.render_chrome(); }
            }
            core::task::Poll::Ready(result) => {
                match result {
                    Ok(loaded) => self.finish_load(loaded, pending.navigation),
                    Err(error) => {
                        self.url_input = self.current_tab().url.clone();
                        self.status_text = format!("Load failed: {}", error);
                    }
                }
                self.render();
            }
        }
    }

    fn finish_load(&mut self, loaded: LoadedDocument, navigation: Navigation) {
        let retained: usize = self.tabs.iter().enumerate()
            .filter(|(i, _)| *i != self.active_tab)
            .map(|(_, t)| t.resident_bytes())
            .sum();
        let decoded_bytes = match loaded.resources.estimated_decoded_bytes() {
            Ok(bytes) => bytes,
            Err(error) => {
                self.status_text = format!("Image budget: {}", error);
                self.url_input = self.current_tab().url.clone();
                return;
            }
        };
        let resource_warning = loaded.resources.warnings().next().map(String::from);
        // Reserve layout/text expansion before parsing; image buffers are shared,
        // but the encoded resource bundle remains owned by this tab.
        let direct_bytes: usize = loaded.direct_nodes.as_ref().map_or(0, |nodes| nodes.iter().map(|n| {
            if let DocNode::Image { pixels, .. } = n { pixels.len() } else { 256 }
        }).sum());
        let reserve = loaded.content.len().saturating_mul(24)
            .saturating_add(loaded.resources.resident_bytes())
            .saturating_add(decoded_bytes)
            .saturating_add(direct_bytes).saturating_add(128 * 1024);
        if retained.saturating_add(reserve) > budget::MAX_RESIDENT_BYTES {
            self.status_text = String::from("Document exceeds the remaining tab memory budget; close another tab.");
            self.url_input = self.current_tab().url.clone();
            return;
        }
        let viewport_w = self.width.saturating_sub(16);
        let viewport_h = self.viewport_height();
        let zoom = self.tabs[self.active_tab].doc.zoom_percent();
        let scripts_enabled = self.tabs[self.active_tab].scripts_enabled;
        let was_explicitly_web = self.tabs[self.active_tab].doc.arena.is_some() && !self.tabs[self.active_tab].doc.is_reader_mode();
        let enable_reader = !was_explicitly_web;
        let reader_column = self.tabs[self.active_tab].doc.reader_width();
        let is_asset_update = self.tabs[self.active_tab].url == loaded.url;
        let existing_scroll = if is_asset_update { self.tabs[self.active_tab].scroll_y } else { 0 };
        let mut candidate = Tab {
            title: String::new(), url: String::new(), doc: Document::new(),
            scroll_y: existing_scroll, history: Vec::new(), history_idx: 0, js_context_id: 0,
            scripts_enabled, resources: resources::ResourceBundle::default(),
            native_dom_dirty: false,
            browser: browser_runtime::DocumentBrowser::new(&loaded.url),
        };
        candidate.browser.session.set_cookies(self.current_tab().browser.session.cookies());
        candidate.doc.set_memory_budget(budget::MAX_RESIDENT_BYTES.saturating_sub(retained)
            .saturating_sub(loaded.resources.resident_bytes()).saturating_sub(loaded.content.len()));
        let (url, format) = candidate.install_loaded(loaded, viewport_w, viewport_h, &mut self.js_runtime, &mut self.fonts);
        let tab = &mut candidate;
        tab.scroll_y = existing_scroll;
        if zoom != tab.doc.zoom_percent() {
            tab.doc.set_zoom(zoom, &mut self.fonts, viewport_w, viewport_h);
        }
        if enable_reader && tab.doc.arena.is_some() {
            tab.doc.set_reader_width(reader_column, &mut self.fonts, viewport_w, viewport_h);
            let previous = resources::replace(core::mem::take(&mut tab.resources));
            if tab.doc.toggle_reader(&mut self.fonts, viewport_w, viewport_h, &tab.url, &tab.title).is_ok() {
                self.status_text = String::from("Reader Mode · Alt+[ / Alt+] changes column width");
            }
            tab.resources = resources::replace(previous);
        }
        if let Some(error) = &tab.doc.last_error {
            use js::JsContext;
            self.js_runtime.reset();
            self.status_text = format!("Document rejected: {}", error);
            self.url_input = self.current_tab().url.clone();
            return;
        }
        if retained.saturating_add(tab.resident_bytes()) > budget::MAX_RESIDENT_BYTES {
            use js::JsContext;
            self.js_runtime.reset();
            self.status_text = String::from("Rendered document exceeds the shared tab memory budget.");
            self.url_input = self.current_tab().url.clone();
            return;
        }
        tab.history = core::mem::take(&mut self.tabs[self.active_tab].history);
        tab.history_idx = self.tabs[self.active_tab].history_idx;
        if !is_asset_update {
            match navigation {
                Navigation::Push => {
                    tab.history.truncate(tab.history_idx + 1);
                    if tab.history.len() == 128 { tab.history.remove(0); }
                    tab.history.push(url.clone());
                    tab.history_idx = tab.history.len() - 1;
                }
                Navigation::Traverse(index) => tab.history_idx = index,
                Navigation::Replace => {
                    if let Some(entry) = tab.history.get_mut(tab.history_idx) { *entry = url.clone(); }
                }
            }
        }
        self.tabs[self.active_tab] = candidate;
        self.url_input = url;
        ostd::io::println(&format!("[ocel] loaded {} ({}, {} items)",
            self.url_input, format, self.current_tab().doc.nodes.len()));
        self.status_text = format!("Loaded {} · {}", format, self.script_status());
        if let Some(warning) = resource_warning {
            self.status_text = format!("Loaded with resource error: {}", warning);
        }
        if self.search_open { self.perform_search(); }
    }

    fn submit_form(&mut self, input: dom_arena::NodeId) {
        // A submit button without a form has no native default action.
        if self.current_tab().doc.form_owner(input).is_none() {
            return;
        }
        match self.current_tab().doc.form_url(input, &self.current_tab().url) {
            Ok(url) => self.navigate_to(&url),
            Err(error) => self.status_text = format!("Form: {}", error),
        }
        self.render();
    }

    fn reserve_active_document(&mut self) {
        let tab = &self.tabs[self.active_tab];
        let metadata = tab.resident_bytes().saturating_sub(tab.doc.resident_bytes());
        let others: usize = self.tabs.iter().enumerate().filter(|(i,_)| *i != self.active_tab)
            .map(|(_,tab)| tab.resident_bytes()).sum();
        let pending = self.pending_load.as_ref().map_or(0, |pending| pending.job.resident_bytes());
        let remaining = budget::MAX_RESIDENT_BYTES.saturating_sub(others)
            .saturating_sub(metadata).saturating_sub(pending);
        self.tabs[self.active_tab].doc.set_memory_budget(remaining);
    }

    fn toggle_reader_mode(&mut self) {
        let w = self.width.saturating_sub(16);
        let h = self.viewport_height();
        self.clear_transient_state();
        self.reserve_active_document();
        let tab = &mut self.tabs[self.active_tab];
        let previous = resources::replace(core::mem::take(&mut tab.resources));
        self.status_text = match tab.doc.toggle_reader(&mut self.fonts, w, h, &tab.url, &tab.title) {
            Ok(true) => String::from("Reader Mode · Alt+[ / Alt+] changes column width"),
            Ok(false) => String::from("Exited Reader Mode"),
            Err(error) => format!("Reader: {}", error),
        };
        tab.resources = resources::replace(previous);
        tab.scroll_y = 0;
        if self.search_open { self.perform_search(); }
        self.render();
    }

    fn script_status(&self) -> &'static str {
        let tab = self.current_tab();
        if !tab.scripts_enabled { "JS off" }
        else if tab.js_context_id != 0 && tab.js_context_id == self.js_runtime.context_id() && self.js_runtime.is_active() { "JS active" }
        else if tab.js_context_id != 0 { "JS inactive — reload to resume" }
        else { "JS idle" }
    }

    fn set_zoom(&mut self, percent: u16) {
        let w = self.width.saturating_sub(16);
        let h = self.viewport_height();
        self.reserve_active_document();
        let tab = &mut self.tabs[self.active_tab];
        let old_height = tab.doc.total_height.max(1);
        let old_scroll = tab.scroll_y;
        tab.doc.set_zoom(percent, &mut self.fonts, w, h);
        tab.scroll_y = ((old_scroll as i64 * tab.doc.total_height as i64) / old_height as i64)
            .clamp(0, (tab.doc.total_height - h as i32).max(0) as i64) as i32;
        if let Some(error) = &tab.doc.last_error { self.status_text = format!("Zoom: {}", error); }
        if self.search_open { self.perform_search(); }
        if self.toc_open { self.toc_entries = self.current_tab().doc.toc(); }
        self.render();
    }

    fn perform_search(&mut self) {
        let query = self.search_query.trim();
        if query.is_empty() {
            self.search_matches.clear();
            self.search_idx = 0;
            self.tabs[self.active_tab].doc.set_search_highlight("", None);
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
        self.tabs[self.active_tab].doc.set_search_highlight(query, if total == 0 { None } else { Some(0) });

        if let Some(y) = first_match {
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.scroll_y = y.saturating_sub(60).max(0);
            }
        }
    }

    fn next_search_match(&mut self, backwards: bool) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_idx = if backwards {
            (self.search_idx + self.search_matches.len() - 1) % self.search_matches.len()
        } else { (self.search_idx + 1) % self.search_matches.len() };
        self.tabs[self.active_tab].doc.set_search_highlight(&self.search_query, Some(self.search_idx));
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

    fn render(&mut self) { self.render_regions(true, true); }

    fn render_chrome(&mut self) { self.render_regions(false, true); }

    fn render_scroll(&mut self) { self.render_regions(true, false); }

    fn render_regions(&mut self, document_dirty: bool, navigation_dirty: bool) {
        let w = self.width;
        let h = self.height;
        let view_h = self.viewport_height();
        let view_y = (TOOLBAR_HEIGHT + TABBAR_HEIGHT) as i32;

        let active_scroll = self.tabs[self.active_tab].scroll_y;
        let max_s = self.max_scroll();
        let script_status = self.script_status();
        if document_dirty {
        self.tabs[self.active_tab].browser.layout_dirty = true;
        let is_reader = self.tabs[self.active_tab].doc.is_reader_mode();
        let bg_color = if is_reader { self.reader_theme.bg_color() } else { theme::BG_DARK };
        fill_rect(&mut self.surface, 0, view_y, w, view_h, bg_color);
        self.tabs[self.active_tab].doc.render_viewport_themed(
            &mut self.fonts,
            &mut self.surface,
            0,
            view_y,
            w.saturating_sub(16),
            view_h,
            active_scroll,
            if is_reader { Some(self.reader_theme) } else { None },
        );
        // Reading Progress Indicator (2px thin bar at top of viewport)
        if is_reader && max_s > 0 {
            let progress_ratio = (active_scroll as f32 / max_s as f32).clamp(0.0, 1.0);
            let prog_w = (w as f32 * progress_ratio) as u32;
            if prog_w > 0 {
                fill_rect(&mut self.surface, 0, view_y, prog_w, 2, self.reader_theme.accent_color());
            }
        }
        // 3. Render Scrollbar on the right edge if content overflows
        // A scroll invalidates the viewport, not the navigation chrome.
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
        }

        // Navigation chrome uses bounded, vertically centered Inter text.
        let fonts=&mut self.fonts;
        let surf=&mut self.surface;
        let regular=FontFace::UiRegular;
        let semibold=FontFace::UiSemibold;
        if navigation_dirty {
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
        let addr_w=w.saturating_sub(250);
        fill_rect(surf,addr_x,8,addr_w,28,theme::BG_INPUT);
        stroke_rect(surf,addr_x,8,addr_w,28,theme::BORDER);
        ui_text(fonts,surf,(addr_x+8,8,addr_w.saturating_sub(16),28),
            &self.url_input,theme::TEXT_PRIMARY,regular,15.0,false);
        let btn_x=addr_x+addr_w as i32+8;
        fill_rect(surf,btn_x,8,38,28,theme::ACCENT_BLUE);
        ui_text(fonts,surf,(btn_x,8,38,28),"Go",Color::rgb(17,17,27),semibold,14.0,true);
        let reader_x = btn_x + 44;
        let reader = tab.doc.is_reader_mode() || tab.doc.web_layout.is_none();
        let btn_w = 32u32;
        if reader {
            fill_rect(surf, reader_x, 8, btn_w, 28, theme::ACCENT_CYAN);
            draw::draw_book_icon(surf, reader_x + 16, 22, Color::rgb(17, 17, 27));
        } else {
            fill_rect(surf, reader_x, 8, btn_w, 28, theme::BG_INPUT);
            stroke_rect(surf, reader_x, 8, btn_w, 28, theme::BORDER);
            draw::draw_globe_icon(surf, reader_x + 16, 22, theme::ACCENT_BLUE);
        }

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
        }

        if document_dirty && self.search_open {
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
        if document_dirty && self.toc_open {
            let panel_w = 300u32.min(w.saturating_sub(16));
            let panel_x = 8;
            let panel_y = view_y + 8;
            let panel_h = view_h.saturating_sub(16);
            fill_rect(surf, panel_x, panel_y, panel_w, panel_h, theme::BG_INPUT);
            stroke_rect(surf, panel_x, panel_y, panel_w, panel_h, theme::ACCENT_BLUE);
            ui_text(fonts,surf,(panel_x+10,panel_y+2,panel_w.saturating_sub(20),30),
                "Contents · Esc closes",theme::ACCENT_CYAN,semibold,15.0,false);
            let rows = panel_h.saturating_sub(36) / 26;
            for (row, entry) in self.toc_entries.iter().skip(self.toc_offset).take(rows as usize).enumerate() {
                let indent = (entry.level.saturating_sub(1) as u32 * 12).min(60);
                ui_text(fonts,surf,(panel_x+10+indent as i32,panel_y+34+row as i32*26,
                    panel_w.saturating_sub(20+indent),26),&entry.title,
                    theme::TEXT_PRIMARY,regular,14.0,false);
            }
        }
        let status_y=h.saturating_sub(STATUS_HEIGHT) as i32;
        fill_rect(surf,0,status_y,w,STATUS_HEIGHT,theme::BG_TOOLBAR);
        fill_rect(surf,0,status_y,w,1,theme::BORDER);
        let pct=if max_s>0{active_scroll*100/max_s}else{100};
        let info=format!("{} · Zoom {}% · {}%", script_status, self.tabs[self.active_tab].doc.zoom_percent(),pct);
        let info_w=fonts.measure(regular,&info,14.0) as u32+16;
        ui_text(fonts,surf,(10,status_y+1,w.saturating_sub(info_w+30),STATUS_HEIGHT-2),
            &self.status_text,theme::TEXT_MUTED,regular,14.0,false);
        ui_text(fonts,surf,(w.saturating_sub(info_w+8) as i32,status_y+1,info_w,STATUS_HEIGHT-2),
            &info,theme::TEXT_MUTED,regular,14.0,true);

        let top_h = TOOLBAR_HEIGHT + TABBAR_HEIGHT;
        if navigation_dirty {
            self.surface.damage(api::display::Rect { x:0, y:0, w, h:top_h });
        }
        self.surface.damage(api::display::Rect { x:0, y:h.saturating_sub(STATUS_HEIGHT) as i32, w, h:STATUS_HEIGHT });
        if document_dirty {
            self.surface.damage(api::display::Rect { x:0, y:view_y, w, h:view_h });
        }
        if let Some((iw, ih, pixels)) = &self.zoomed_image {
            fill_rect(&mut self.surface, 0, 0, w, h, Color::rgba(0, 0, 0, 220));
            let max_w = w.saturating_sub(64).max(64);
            let max_h = h.saturating_sub(80).max(64);
            let scale_w = max_w as f32 / (*iw).max(1) as f32;
            let scale_h = max_h as f32 / (*ih).max(1) as f32;
            let scale = scale_w.min(scale_h).min(2.0);
            let target_w = ((*iw as f32) * scale) as u32;
            let target_h = ((*ih as f32) * scale) as u32;
            let target_x = ((w.saturating_sub(target_w)) / 2) as i32;
            let target_y = ((h.saturating_sub(target_h)) / 2) as i32;
            draw::draw_image_clipped(&mut self.surface, target_x, target_y, target_w, target_h, pixels, (*iw * 4) as usize, *ih, (0, 0, w as i32, h as i32));
            ui_text(fonts, &mut self.surface, (0, 16, w, 24), "Image Zoom · Click or Escape to close", Color::rgb(240, 240, 240), FontFace::UiSemibold, 15.0, true);
            self.surface.damage(api::display::Rect { x: 0, y: 0, w, h });
        }
    }

    fn handle_mouse_move(&mut self, x: i32, y: i32) {
        self.cursor_x = x;
        self.cursor_y = y;
    }

    fn handle_mouse_scroll(&mut self, dy: i32) {
        if self.toc_open {
            self.toc_offset = (self.toc_offset as i32 - dy).clamp(0, self.toc_entries.len().saturating_sub(1) as i32) as usize;
            self.render();
            return;
        }
        self.scroll_by(-dy * 32);
        self.render_scroll();
    }

    fn handle_mouse_button(&mut self, button: MouseButton, state: KeyState) {
        if button != MouseButton::Left || state != KeyState::Pressed {
            return;
        }
        if self.zoomed_image.is_some() {
            self.zoomed_image = None;
            self.render();
            return;
        }

        let cx = self.cursor_x;
        let cy = self.cursor_y;
        let w = self.width;
        let view_y = (TOOLBAR_HEIGHT + TABBAR_HEIGHT) as i32;
        let view_h = self.viewport_height() as i32;
        if self.toc_open && (8..308.min(w as i32 - 8)).contains(&cx)
            && (view_y + 8..view_y + view_h - 8).contains(&cy) {
            if cy >= view_y + 42 {
                let row = ((cy - view_y - 42) / 26) as usize + self.toc_offset;
                if let Some(entry) = self.toc_entries.get(row) {
                    let y = entry.y;
                    self.current_tab_mut().scroll_y = y.clamp(0, self.max_scroll());
                    self.toc_open = false;
                    self.render();
                }
            }
            return;
        }

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
        let addr_w = w.saturating_sub(250) as i32;
        let btn_x = addr_x + addr_w + 8;
        if (btn_x..btn_x + 38).contains(&cx) && (8..36).contains(&cy) {
            let target = loader::classify_smart_url(&self.url_input);
            self.navigate_to(&target);
            self.render();
            return;
        }
        let reader_x = btn_x + 44;
        if (reader_x..reader_x + 32).contains(&cx) && (8..36).contains(&cy) {
            self.toggle_reader_mode();
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
            let active_tab_idx = self.active_tab;
            let active_scroll = self.tabs[active_tab_idx].scroll_y;

            // Script listeners run before native form/link default actions.
            self.dispatch_document_click(cx, cy);
            let hit_input = self.tabs[active_tab_idx].doc.hit_test_input_node(cx, cy, 0, view_y, active_scroll);
            if let Some(id) = hit_input {
                let input_type = self.tabs[active_tab_idx].doc.input(id).map(|input| input.input_type.clone());
                if !matches!(input_type.as_deref(), Some("text"|"textarea"|"search"|"email"|"url"|"password")) {
                    self.focused_input = None;
                    self.tabs[active_tab_idx].doc.focus_input(None);
                }
                match input_type.as_deref() {
                    Some("checkbox") => {
                        let tab = &mut self.tabs[active_tab_idx];
                        if tab.doc.toggle_input(id) { tab.native_dom_dirty = true; }
                        self.render();
                        return;
                    }
                    Some("submit") => { self.submit_form(id); return; }
                    Some("button") => self.render(),
                    Some("text") | Some("textarea") | Some("search") | Some("email") | Some("url") | Some("password") => {
                        self.focused_input = Some(id);
                        self.tabs[active_tab_idx].doc.focus_input(Some(id));
                        self.render();
                        return;
                    }
                    None => {}
                    Some(_) => {
                        self.status_text = String::from("This form control type is not supported.");
                        self.render();
                        return;
                    }
                }
            } else if self.focused_input.take().is_some() {
                self.tabs[active_tab_idx].doc.focus_input(None);
                self.render();
            }

            // Check Hyperlink navigation
            let hit_link = self.tabs[active_tab_idx].doc.hit_test_link(cx, cy, 0, view_y, active_scroll);
            if let Some(link_url) = hit_link {
                ostd::io::print("[ocel] Clicked link -> navigating to: ");
                ostd::io::println(&link_url);
                self.navigate_to(&link_url);
                self.render();
                return;
            }
            // Check Image Click to Zoom
            if let Some(img) = self.tabs[active_tab_idx].doc.hit_test_image(cx, cy, 0, view_y, active_scroll) {
                self.zoomed_image = Some(img);
                self.render();
                return;
            }

        }
    }

    fn dispatch_document_click(&mut self, cx: i32, cy: i32) {
        let active_tab_idx = self.active_tab;
        let active_scroll = self.tabs[active_tab_idx].scroll_y;
        let view_y = (TOOLBAR_HEIGHT + TABBAR_HEIGHT) as i32;
        let view_h = self.viewport_height() as i32;
            let hit_node = self.tabs[active_tab_idx].doc.hit_test_node(cx, cy, 0, view_y, active_scroll);
            if let Some(node_id) = hit_node {
                let active_tab = &self.tabs[active_tab_idx];
                if !active_tab.scripts_enabled || active_tab.doc.is_reader_mode() { return; }
                if active_tab.js_context_id == 0 || active_tab.js_context_id != self.js_runtime.context_id() || !self.js_runtime.is_active() {
                    self.status_text = String::from("JavaScript inactive or unavailable; Ctrl+J changes script policy, F5 reloads.");
                    self.render_chrome();
                    return;
                }
                // Native edits are committed to the arena. Refresh the engine's
                // snapshot before dispatch without replacing its listeners.
                if active_tab.native_dom_dirty {
                    if let Some(arena) = &active_tab.doc.arena {
                        if let Err(error) = self.js_runtime.sync_document(arena, &active_tab.title) {
                            self.status_text = format!("JavaScript synchronization failed: {}", error.message);
                            self.render_chrome();
                            return;
                        }
                    }
                    self.tabs[active_tab_idx].native_dom_dirty = false;
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

                let viewport_w = self.width.saturating_sub(16);
                self.reserve_active_document();
                let tab = &mut self.tabs[active_tab_idx];
                if let Err(error) = tab.dispatch_browser_event(&mut self.js_runtime, &event, &mut self.fonts, viewport_w, view_h as u32) {
                    self.status_text = format!("JavaScript: {}", error.message);
                    ostd::io::println(&format!("[ocel] {}", self.status_text));
                }
                if let Err(error) = tab.drain_browser_output(&mut self.js_runtime, &mut self.fonts, viewport_w, view_h as u32) {
                    use js::JsContext;
                    tab.browser.cancel();
                    self.js_runtime.reset();
                    tab.js_context_id = 0;
                    self.status_text = format!("JavaScript document update rejected: {error}");
                }
                self.render();
            }
    }

    fn handle_key(&mut self, sym: KeySym, ch: Option<char>, modifiers: Modifiers) {
        if sym == KeySym::Escape && self.pending_load.is_some() {
            self.cancel_loading();
            self.render_chrome();
            return;
        }
        if sym == KeySym::F5 {
            let url = self.current_tab().url.clone();
            self.load_url(&url);
            self.render_chrome();
            return;
        }
        if modifiers.contains(Modifiers::CTRL) {
            match ch {
                Some('+') | Some('=') => { self.set_zoom(self.current_tab().doc.zoom_percent().saturating_add(10)); return; }
                Some('-') => { self.set_zoom(self.current_tab().doc.zoom_percent().saturating_sub(10)); return; }
                Some('0') => { self.set_zoom(100); return; }
                Some('m') => {
                    self.toc_open = !self.toc_open;
                    if self.toc_open {
                        self.toc_entries = self.current_tab().doc.toc();
                        self.toc_offset = 0;
                        self.status_text = if self.toc_entries.is_empty() { String::from("No headings in this document") }
                            else { String::from("Contents · click a heading, wheel or arrows to browse") };
                    }
                    self.render();
                    return;
                }
                Some('j') => {
                    use js::JsContext;
                    self.current_tab_mut().scripts_enabled = !self.current_tab().scripts_enabled;
                    self.js_runtime.reset();
                    self.current_tab_mut().js_context_id = 0;
                    self.current_tab_mut().browser.cancel();
                    self.status_text = if self.current_tab().scripts_enabled {
                        String::from("Scripts allowed for this tab; F5 reloads and executes")
                    } else { String::from("Scripts disabled for this tab; F5 reloads a clean static document") };
                    self.render_chrome();
                    return;
                }
                _ => {}
            }
        }
        if modifiers.contains(Modifiers::ALT) && self.current_tab().doc.is_reader_mode() {
            let delta = match ch { Some('[') => -80, Some(']') => 80, _ => 0 };
            if delta != 0 {
                let width = (self.current_tab().doc.reader_width() as i32 + delta).clamp(320,1200) as u32;
                let w = self.width.saturating_sub(16);
                let h = self.viewport_height();
                self.reserve_active_document();
                self.tabs[self.active_tab].doc.set_reader_width(width, &mut self.fonts, w, h);
                if let Some(error) = &self.tabs[self.active_tab].doc.last_error {
                    self.status_text = format!("Reader width: {}", error);
                }
                if self.search_open { self.perform_search(); }
                self.render();
                return;
            }
        }
        if self.toc_open {
            match sym {
                KeySym::Escape => self.toc_open = false,
                KeySym::Up => self.toc_offset = self.toc_offset.saturating_sub(1),
                KeySym::Down => self.toc_offset = (self.toc_offset + 1).min(self.toc_entries.len().saturating_sub(1)),
                KeySym::Return => {
                    if let Some(entry) = self.toc_entries.get(self.toc_offset) {
                        let y = entry.y;
                        self.current_tab_mut().scroll_y = y.clamp(0, self.max_scroll());
                    }
                    self.toc_open = false;
                }
                _ => return,
            }
            self.render();
            return;
        }
        let is_ctrl = |target: char| {
            ch == Some(target) && modifiers.contains(Modifiers::CTRL)
        };
        let is_alt = |target: char| {
            ch == Some(target) && modifiers.contains(Modifiers::ALT)
        };
        // Global Ctrl+T: New Tab
        if is_ctrl('t') {
            self.new_tab("file:///welcome.md");
            self.render();
            return;
        }

        // Global Ctrl+W: Close Tab (Exit Ocel if last tab)
        if is_ctrl('w') {
            if self.tabs.len() <= 1 {
                self.should_exit = true;
                return;
            }
            self.close_tab(self.active_tab);
            self.render();
            return;
        }

        // Global Ctrl+Q: Exit Ocel and return to shell
        if is_ctrl('q') {
            self.should_exit = true;
            return;
        }

        if sym == KeySym::Escape && self.zoomed_image.is_some() {
            self.zoomed_image = None;
            self.render();
            return;
        }

        // Global Escape: Exit Ocel when no dialog/search/input is active
        if sym == KeySym::Escape && !self.search_open && self.pending_load.is_none() && self.focused_input.is_none() && !self.toc_open {
            self.should_exit = true;
            return;
        }
        // Global Ctrl+R: Toggle Reader Mode
        if is_ctrl('r') {
            self.toggle_reader_mode();
            return;
        }
        // Global Alt+T: Cycle Reader Theme
        if is_alt('t') {
            self.reader_theme = self.reader_theme.next();
            self.status_text = format!("Reader Theme: {} · Alt+T to cycle", self.reader_theme.name());
            self.render();
            return;
        }
        // Global Alt+B: Toggle Bionic Reading Mode
        if is_alt('b') {
            let width = self.width.saturating_sub(16);
            let state = self.tabs[self.active_tab].doc.toggle_bionic(&mut self.fonts, width);
            self.status_text = format!("Bionic Reading: {} · Alt+B to toggle", if state { "ON" } else { "OFF" });
            self.render();
            return;
        }
        // Global Alt+F: Toggle Reader Font (Sans-serif vs Monospace)
        if is_alt('f') {
            let width = self.width.saturating_sub(16);
            let state = self.tabs[self.active_tab].doc.toggle_mono(&mut self.fonts, width);
            self.status_text = format!("Reader Font: {} · Alt+F to toggle", if state { "Tech Monospace (JetBrains)" } else { "Modern Sans-serif (Inter)" });
            self.render();
            return;
        }
        // Global Alt+H: Save Highlight to /data/highlights.md
        if is_alt('h') {
            let tab = self.current_tab();
            let url = tab.url.clone();
            let title = if tab.title.is_empty() { &url } else { &tab.title };
            let entry = format!("\n- Highlight: [{}]({})\n", title, url);
            let saved = if let Ok(mut file) = ostd::fs::File::open("/data/highlights.md") {
                file.write_all(entry.as_bytes()).is_ok()
            } else {
                false
            };
            if saved {
                self.status_text = format!("Saved highlight for {} to /data/highlights.md", title);
            } else {
                self.status_text = format!("Highlighted: {} (session)", title);
            }
            self.render_chrome();
            return;
        }
        // Global Ctrl+L / F6: Focus Address Bar
        if is_ctrl('l') || sym == KeySym::F6 {
            self.url_input.clear();
            self.status_text = String::from("Address bar ready: type a URL or keywords, Enter to go.");
            self.render_chrome();
            return;
        }


        // Global Ctrl+Tab: Next Tab
        if sym == KeySym::Tab && modifiers.contains(Modifiers::CTRL) {
            self.next_tab();
            self.render();
            return;
        }
        // Global Alt+Left: Go Back
        if sym == KeySym::Left && modifiers.contains(Modifiers::ALT) {
            self.go_back();
            self.render();
            return;
        }

        // Global Alt+Right: Go Forward
        if sym == KeySym::Right && modifiers.contains(Modifiers::ALT) {
            self.go_forward();
            self.render();
            return;
        }

        // Global Ctrl+B: Open Bookmarks
        if is_ctrl('b') {
            self.navigate_to("ocel://bookmarks");
            self.render();
            return;
        }

        // Global Ctrl+D: Bookmark Current Page
        if is_ctrl('d') {
            let tab = self.current_tab();
            let url = tab.url.clone();
            let title = if tab.title.is_empty() { &url } else { &tab.title };
            let entry = format!("\n- [{}]({})", title, url);
            let saved = append_file_vfs("/data/bookmarks.md", entry.as_bytes());
            if saved {
                self.status_text = format!("Saved bookmark: {} (Ctrl+B to view)", title);
            } else {
                self.status_text = format!("Bookmarked: {} (session)", title);
            }
            self.render_chrome();
            return;
        }

        // Global Ctrl+S: Save Page Content to Markdown (.md) or Plain Text (.txt)
        if is_ctrl('s') {
            let tab = self.current_tab();
            let title = if tab.title.is_empty() { "document" } else { &tab.title };
            let slug: String = title.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
            let slug = if slug.is_empty() { String::from("document") } else { slug };

            let save_as_text = modifiers.contains(Modifiers::SHIFT)
                || tab.url.ends_with(".txt")
                || tab.url.ends_with(".log");

            let (ext, content) = if save_as_text {
                ("txt", export_tab_text(tab))
            } else {
                ("md", export_tab_markdown(tab))
            };

            let path = format!("/data/saved-{}.{}", slug, ext);
            let saved = save_file_vfs(&path, content.as_bytes());
            if saved {
                self.status_text = format!("Saved {}: {} ({} bytes)", ext.to_ascii_uppercase(), path, content.len());
            } else {
                self.status_text = format!("Exported {}: {} ({} bytes in memory)", ext.to_ascii_uppercase(), title, content.len());
            }
            self.render_chrome();
            return;
        }


        if sym == KeySym::F3 || is_ctrl('f') {
            if sym == KeySym::F3 && self.search_open {
                self.next_search_match(modifiers.contains(Modifiers::SHIFT));
            } else {
                self.search_open = true;
                self.perform_search();
            }
            self.render();
            return;
        }

        // Escape: Close search bar
        if sym == KeySym::Escape && self.search_open {
            self.search_open = false;
            self.tabs[self.active_tab].doc.set_search_highlight("", None);
            self.render();
            return;
        }

        // In search mode
        if self.search_open {
            match sym {
                KeySym::Return => {
                    self.next_search_match(modifiers.contains(Modifiers::SHIFT));
                    self.render();
                }
                KeySym::Backspace => {
                    self.search_query.pop();
                    self.perform_search();
                    self.render();
                }
                _ => {
                    if let Some(c) = ch {
                        if !c.is_control() && self.search_query.len() < 256 {
                            self.search_query.push(c);
                            self.perform_search();
                            self.render();
                        }
                    }
                }
            }
            return;
        }

        if let Some(id) = self.focused_input {
            self.reserve_active_document();
            match sym {
                KeySym::Return => {
                    if self.current_tab().doc.input(id).is_some_and(|input|input.input_type=="textarea") {
                        let tab = self.current_tab_mut();
                        if tab.doc.edit_input(id, '\n') { tab.native_dom_dirty = true; }
                    } else { self.submit_form(id); return; }
                }
                KeySym::Escape => {
                    self.focused_input = None;
                    self.current_tab_mut().doc.focus_input(None);
                }
                KeySym::Backspace => {
                    let tab = self.current_tab_mut();
                    if tab.doc.edit_input(id, '\u{8}') { tab.native_dom_dirty = true; }
                }
                _ => {
                    if let Some(c) = ch.filter(|c| !c.is_control()) {
                        let tab = self.current_tab_mut();
                        if tab.doc.edit_input(id, c) { tab.native_dom_dirty = true; }
                    }
                }
            }
            self.render();
            return;
        }

        // Standard Document Navigation
        match sym {
            KeySym::Return => {
                let target = loader::classify_smart_url(&self.url_input);
                self.navigate_to(&target);
                self.render();
            }
            KeySym::Backspace => {
                self.url_input.pop();
                self.render_chrome();
            }
            KeySym::Up => {
                self.scroll_by(-24);
                self.render_scroll();
            }
            KeySym::Down => {
                self.scroll_by(24);
                self.render_scroll();
            }
            KeySym::PageUp => {
                let step = (self.viewport_height() as i32).saturating_sub(48);
                self.scroll_by(-step);
                self.render_scroll();
            }
            KeySym::PageDown => {
                let step = (self.viewport_height() as i32).saturating_sub(48);
                self.scroll_by(step);
                self.render_scroll();
            }
            KeySym::Home => {
                self.current_tab_mut().scroll_y = 0;
                self.render_scroll();
            }
            KeySym::End => {
                let max_s = self.max_scroll();
                self.current_tab_mut().scroll_y = max_s;
                self.render_scroll();
            }
            _ => {
                if let Some(c) = ch {
                    if c == ' ' {
                        let step = (self.viewport_height() as i32).saturating_sub(48);
                        if modifiers.contains(Modifiers::SHIFT) {
                            self.scroll_by(-step);
                        } else {
                            self.scroll_by(step);
                        }
                        self.render_scroll();
                    } else if !c.is_control() && self.url_input.len() < 2048 {
                        self.url_input.push(c);
                        self.render_chrome();
                    }
                }
            }
        }
    }
}

fn export_tab_markdown(tab: &Tab) -> String {
    let mut out = String::new();
    if !tab.title.is_empty() {
        out.push_str(&format!("# {}\n\n", tab.title));
    }
    for node in &tab.doc.nodes {
        match node {
            DocNode::Heading { level, text } => {
                let hashes = "#".repeat((*level as usize).clamp(1, 6));
                out.push_str(&format!("{} {}\n\n", hashes, text));
            }
            DocNode::Paragraph { spans } => {
                for s in spans {
                    if let Some(link) = &s.link {
                        out.push_str(&format!("[{}]({})", s.text, link));
                    } else if s.bold {
                        out.push_str(&format!("**{}**", s.text));
                    } else if s.italic {
                        out.push_str(&format!("*{}*", s.text));
                    } else if s.code {
                        out.push_str(&format!("`{}`", s.text));
                    } else {
                        out.push_str(&s.text);
                    }
                }
                out.push_str("\n\n");
            }
            DocNode::ListItem { spans, .. } => {
                out.push_str("- ");
                for s in spans {
                    if let Some(link) = &s.link {
                        out.push_str(&format!("[{}]({})", s.text, link));
                    } else {
                        out.push_str(&s.text);
                    }
                }
                out.push('\n');
            }
            DocNode::CodeBlock { lang, lines } => {
                out.push_str(&format!("```{}\n", lang));
                for line in lines {
                    out.push_str(line);
                    out.push('\n');
                }
                out.push_str("```\n\n");
            }
            DocNode::Blockquote { lines } => {
                for line in lines {
                    out.push_str(&format!("> {}\n", line));
                }
                out.push('\n');
            }
            DocNode::Rule => {
                out.push_str("---\n\n");
            }
            _ => {}
        }
    }
    out
}

fn save_file_vfs(path: &str, content: &[u8]) -> bool {
    let mut vfs = ostd::clients::vfs::VfsClient::new();
    if vfs.write_file(path, content).is_ok() {
        return true;
    }
    if let Ok(mut f) = ostd::fs::File::open(path) {
        return f.write_all(content).is_ok();
    }
    false
}

fn append_file_vfs(path: &str, content: &[u8]) -> bool {
    let mut vfs = ostd::clients::vfs::VfsClient::new();
    if vfs.append_file(path, content).is_ok() {
        return true;
    }
    if let Ok(mut f) = ostd::fs::File::open(path) {
        return f.write_all(content).is_ok();
    }
    false
}

fn export_tab_text(tab: &Tab) -> String {
    let mut out = String::new();
    if !tab.title.is_empty() {
        out.push_str(&tab.title);
        out.push_str("\n\n");
    }
    for node in &tab.doc.nodes {
        match node {
            DocNode::Heading { text, .. } => {
                out.push_str(text);
                out.push_str("\n\n");
            }
            DocNode::Paragraph { spans } => {
                for s in spans {
                    out.push_str(&s.text);
                }
                out.push_str("\n\n");
            }
            DocNode::ListItem { spans, bullet, .. } => {
                if *bullet != '\0' {
                    out.push(*bullet);
                    out.push(' ');
                }
                for s in spans {
                    out.push_str(&s.text);
                }
                out.push('\n');
            }
            DocNode::CodeBlock { lines, .. } => {
                for l in lines {
                    out.push_str(l);
                    out.push('\n');
                }
                out.push('\n');
            }
            DocNode::Blockquote { lines } | DocNode::RawLines { lines } => {
                for l in lines {
                    out.push_str(l);
                    out.push('\n');
                }
                out.push('\n');
            }
            DocNode::Rule => {
                out.push_str("----------------------------------------\n\n");
            }
            _ => {}
        }
    }
    out
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
    viewer.surface.raise();

    while !ostd::input::request_focus() {
        sys_yield();
    }

    viewer.render();
    ostd::io::println("[ocel] Window initialized and painted.");

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
        if viewer.should_exit {
            break;
        }
        loader::poll_cleanup();
        viewer.poll_loading();
        viewer.poll_browser();

        sys_yield();
    }

    ostd::input::release_focus();
    drop(viewer);
    ostd::io::println("[ocel] Exited cleanly.");
}

#[cfg(test)]
mod main_ux_tests {
    use super::*;

    #[test]
    fn test_reader_theme_mode_cycling_and_properties() {
        use draw::ReaderThemeMode;
        assert_eq!(ReaderThemeMode::Dark.next(), ReaderThemeMode::Sepia);
        assert_eq!(ReaderThemeMode::Sepia.next(), ReaderThemeMode::PaperWhite);
        assert_eq!(ReaderThemeMode::PaperWhite.next(), ReaderThemeMode::Dark);

        assert_eq!(ReaderThemeMode::Dark.name(), "Dark");
        assert_eq!(ReaderThemeMode::Sepia.name(), "Sepia");
        assert_eq!(ReaderThemeMode::PaperWhite.name(), "Paper White");

        assert_ne!(ReaderThemeMode::Dark.bg_color(), ReaderThemeMode::Sepia.bg_color());
        assert_ne!(ReaderThemeMode::Sepia.text_color(), ReaderThemeMode::PaperWhite.text_color());
    }

    #[test]
    fn test_export_tab_markdown() {
        let mut tab = Tab {
            title: String::from("Sample Article"),
            url: String::from("https://example.com/test"),
            doc: Document::new(),
            scroll_y: 0,
            history: Vec::new(),
            history_idx: 0,
            js_context_id: 0,
            scripts_enabled: false,
            resources: resources::ResourceBundle::default(),
            native_dom_dirty: false,
            browser: browser_runtime::DocumentBrowser::new("https://example.com/test"),
        };
        let mut link_span = doc::StyledSpan::plain("Link Text");
        link_span.link = Some(String::from("https://example.com/target"));
        let mut bold_span = doc::StyledSpan::plain("Bold Text");
        bold_span.bold = true;
        tab.doc.nodes = alloc::vec![
            DocNode::Heading { level: 2, text: String::from("Introduction") },
            DocNode::Paragraph { spans: alloc::vec![link_span, doc::StyledSpan::plain(" and "), bold_span] },
            DocNode::CodeBlock { lang: String::from("rust"), lines: alloc::vec![String::from("fn main() {}")] },
            DocNode::Rule,
        ];
        let md = export_tab_markdown(&tab);
        assert!(md.contains("# Sample Article"));
        assert!(md.contains("## Introduction"));
        assert!(md.contains("[Link Text](https://example.com/target) and **Bold Text**"));
        assert!(md.contains("```rust\nfn main() {}\n```"));
        assert!(md.contains("---"));
    }

    #[test]
    fn test_bionic_reading_algorithm() {
        let input = alloc::vec![doc::StyledSpan::plain("CellOS microkernel architecture")];
        let bionic = doc::apply_bionic_reading(&input);
        assert!(bionic.iter().any(|s| s.text == "Cel" && s.bold));
        assert!(bionic.iter().any(|s| s.text == "lOS" && !s.bold));
        assert!(bionic.iter().any(|s| s.text == "microk" && s.bold));
        assert!(bionic.iter().any(|s| s.text == "ernel" && !s.bold));
    }

    #[test]
    fn test_export_tab_text() {
        let mut tab = Tab {
            title: String::from("Sample Article"),
            url: String::from("https://example.com/test"),
            doc: Document::new(),
            scroll_y: 0,
            history: Vec::new(),
            history_idx: 0,
            js_context_id: 0,
            scripts_enabled: false,
            resources: resources::ResourceBundle::default(),
            native_dom_dirty: false,
            browser: browser_runtime::DocumentBrowser::new("https://example.com/test"),
        };
        tab.doc.nodes = alloc::vec![
            DocNode::Heading { level: 2, text: String::from("Introduction") },
            DocNode::Paragraph { spans: alloc::vec![doc::StyledSpan::plain("Plain text without markdown markup.")] },
        ];
        let text = export_tab_text(&tab);
        assert!(text.contains("Sample Article\n\nIntroduction"));
        assert!(text.contains("Plain text without markdown markup."));
        assert!(!text.contains("##"));
    }
}
