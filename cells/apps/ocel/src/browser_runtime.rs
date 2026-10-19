// SPDX-License-Identifier: MIT
//! Viewer-side browser scheduling. Every completion belongs to one document token.
use super::*;
use alloc::collections::VecDeque;
use dom_arena::{BrowserEnvironment, BrowserFrame, BrowserReply, BrowserRequest, BrowserResourceKind, JsContext};

pub(crate) struct DocumentBrowser {
    pub session: browser::BrowserSession,
    pub layout_dirty: bool,
    pub frame_pending: bool,
    replies: VecDeque<BrowserReply>,
}
impl DocumentBrowser {
    pub fn new(url: &str) -> Self {
        Self { session: browser::BrowserSession::new(url), layout_dirty: true, frame_pending: true, replies: VecDeque::new() }
    }
    pub fn cancel(&mut self) { self.session.cancel_all(); self.replies.clear(); self.frame_pending = false; }
    pub fn resident_bytes(&self) -> usize {
        self.session.resident_bytes() + self.replies.capacity() * core::mem::size_of::<BrowserReply>()
            + self.replies.iter().map(|reply| reply.url.capacity() + reply.body.capacity()
                + reply.error.as_ref().map_or(0, String::capacity)
                + reply.headers.capacity() * core::mem::size_of::<(String,String)>()
                + reply.headers.iter().map(|(name,value)| name.capacity()+value.capacity()).sum::<usize>()).sum::<usize>()
    }
}

impl Tab {
    pub(crate) fn configure_browser(&mut self, runtime: &mut js::Tier2JsBridge, width: u32, height: u32) -> Result<(), String> {
        let now = ostd::syscall::sys_get_time_ms().ok_or("Browser monotonic clock unavailable")?;
        runtime.configure_browser(&BrowserEnvironment {
            url: self.url.clone(), user_agent: "Ocel/0.2 (CellOS)".into(), width, height, now_ms: now,
            cookie: self.browser.session.document_cookie(),
        }).map_err(|error| error.message)
    }

    fn native_browser_layout(&mut self, mutations: &[dom_arena::DomMutation], node: dom_arena::NodeId,
        fonts: &mut TextFonts, width: u32, height: u32, parsing_complete: bool) -> Result<BrowserFrame, dom_arena::JsError> {
        let result = (|| -> Result<BrowserFrame,String> {
            if !mutations.is_empty() {
                self.doc.apply_mutations_checked(mutations)?;
                for mutation in mutations {
                    if let dom_arena::DomMutation::SetDocumentTitle { title } = mutation { self.title = title.clone(); }
                }
            }
            if !self.doc.relayout_from_arena(fonts, width, height) {
                return Err(self.doc.last_error.clone().unwrap_or_else(|| "Synchronous browser layout refused".into()));
            }
            let arena = self.doc.arena.as_ref().ok_or("Synchronous layout document unavailable")?;
            if !arena.get(node).is_some_and(|element| element.is_element()) { return Err("Synchronous layout query requires an element".into()); }
            let layout = alloc::vec![self.doc.web_layout.as_ref().ok_or("Synchronous layout unavailable")?
                .browser_node_layout(arena,&self.doc.external_stylesheets,node,width,height,self.scroll_y)?];
            self.browser.layout_dirty = true;
            Ok(BrowserFrame {
                now_ms: ostd::syscall::sys_get_time_ms().ok_or("Browser monotonic clock unavailable")?,
                width, height, scroll_y: self.scroll_y, layout, cookie: self.browser.session.document_cookie(), parsing_complete,
            })
        })();
        result.map_err(|message| dom_arena::JsError { message, line: 0 })
    }

    pub(crate) fn eval_browser_script(&mut self, runtime: &mut js::Tier2JsBridge, source: &str, node: dom_arena::NodeId,
        fonts: &mut TextFonts, width: u32, height: u32) -> Result<String, dom_arena::JsError> {
        runtime.eval_script_with_layout(source, Some(node), &mut |mutations,node| {
            self.native_browser_layout(mutations,node,fonts,width,height,false)
        })
    }

    pub(crate) fn dispatch_browser_event(&mut self, runtime: &mut js::Tier2JsBridge, event: &dom_arena::DomEvent,
        fonts: &mut TextFonts, width: u32, height: u32) -> Result<(), dom_arena::JsError> {
        runtime.dispatch_event_with_layout(event, &mut |mutations,node| {
            self.native_browser_layout(mutations,node,fonts,width,height,true)
        })
    }

    /// Drain mutations before any further engine command; an invalid batch closes
    /// the context instead of allowing native and engine DOMs to diverge.
    pub(crate) fn drain_browser_output(&mut self, runtime: &mut js::Tier2JsBridge, fonts: &mut TextFonts, width: u32, height: u32) -> Result<bool, String> {
        let mutations = runtime.take_mutations();
        let changed = !mutations.is_empty();
        if changed {
            self.doc.apply_mutations_checked(&mutations)?;
            let layout_changed = mutations.iter().any(|mutation| !matches!(mutation, dom_arena::DomMutation::SetDocumentTitle { .. }));
            for mutation in mutations {
                if let dom_arena::DomMutation::SetDocumentTitle { title } = mutation { self.title = title; }
            }
            if layout_changed {
                if !self.doc.relayout_from_arena(fonts, width, height) {
                    return Err(self.doc.last_error.clone().unwrap_or_else(|| "Browser layout failed".into()));
                }
                self.browser.layout_dirty = true;
            }
        }
        for request in runtime.take_browser_requests() {
            let id = match &request { BrowserRequest::Load { id, .. } | BrowserRequest::Fetch { id, .. } => Some(*id), _ => None };
            self.browser.frame_pending = true;
            if let Err(error) = self.browser.session.enqueue(request) {
                ostd::io::println(&format!("[ocel] browser resource refused: {error}"));
                if let Some(id) = id {
                    if self.browser.replies.len() >= dom_arena::OCEL_JS_MAX_BROWSER_REQUESTS { return Err("Browser completion queue budget exceeded".into()); }
                    self.browser.replies.push_back(BrowserReply { id, url: String::new(), status: 0, headers: Vec::new(), body: String::new(), error: Some(error) });
                }
            }
        }
        Ok(changed)
    }

    pub(crate) fn pump_browser_frame(&mut self, runtime: &mut js::Tier2JsBridge, fonts: &mut TextFonts, width: u32, height: u32, parsing_complete: bool) -> Result<(), String> {
        let now = ostd::syscall::sys_get_time_ms().ok_or("Browser monotonic clock unavailable")?;
        let mut layout = if self.browser.layout_dirty {
            match (&self.doc.web_layout,&self.doc.arena) {
                (Some(web),Some(arena)) => web.browser_layout(arena,self.scroll_y),
                _ => Vec::new(),
            }
        } else { Vec::new() };
        // Keep common native styles when the snapshot fits; otherwise CSSOM
        // obtains computed styles through the same synchronous native query.
        if dom_arena::ipc::serialized_len_bounded(&layout, dom_arena::OCEL_JS_MAX_TRANSFER / 2).is_err() {
            for node in &mut layout { node.styles.clear(); }
        }
        self.browser.layout_dirty = false;
        self.browser.frame_pending = false;
        let frame = BrowserFrame { now_ms: now, width, height, scroll_y: self.scroll_y, layout,
            cookie: self.browser.session.document_cookie(), parsing_complete };
        runtime.pump_browser_with_layout(&frame, &mut |mutations,node| {
            self.native_browser_layout(mutations,node,fonts,width,height,parsing_complete)
        }).map_err(|error| error.message)
    }

    fn install_browser_resource(&mut self, completion: &mut browser::BrowserCompletion, fonts: &mut TextFonts, width: u32, height: u32, memory_limit: usize) -> Result<bool, String> {
        let BrowserRequest::Load { node, url, kind, .. } = &completion.request else { return Ok(false); };
        let id = node.as_ref().ok_or("Dynamic resource requires its native DOM node")?;
        let arena = self.doc.arena.as_ref().ok_or("Resource document no longer exists")?;
        let node = arena.get(*id).ok_or("Resource node no longer exists")?;
        let (tag, attr) = match kind { BrowserResourceKind::Script => ("script", "src"), BrowserResourceKind::Stylesheet => ("link", "href"), BrowserResourceKind::Image => ("img", "src") };
        let mut cursor = Some(*id);
        let mut connected = false;
        while let Some(id) = cursor { if id == arena.root { connected = true; break; } cursor = arena.get(id).and_then(|node| node.parent); }
        if (!connected && kind != &BrowserResourceKind::Image) || node.tag() != Some(tag) || kind == &BrowserResourceKind::Stylesheet && !node.get_attribute("rel").is_some_and(|rel| rel.split_ascii_whitespace().any(|word| word.eq_ignore_ascii_case("stylesheet"))) {
            return Err("Resource node is no longer active".into());
        }
        let current = resources::resolve(&self.url, node.get_attribute(attr).ok_or("Resource URL was removed")?)?;
        if current != resources::resolve(&self.url, url)? { return Err("Resource node URL changed while loading".into()); }
        if completion.reply.error.is_some() { return Ok(false); }
        if !(200..300).contains(&completion.reply.status) { return Err(format!("Resource HTTP status {}", completion.reply.status)); }
        match kind {
            BrowserResourceKind::Script => Ok(false),
            BrowserResourceKind::Stylesheet => {
                crate::budget::validate_css(&completion.reply.body)?;
                if completion.reply.body.capacity().saturating_add(self.doc.resident_bytes()) > memory_limit {
                    return Err("Dynamic stylesheet exceeds remaining tab memory budget".into());
                }
                let index = self.doc.external_stylesheets.iter().position(|(node,_)| node == id);
                let previous = index.map(|index| self.doc.external_stylesheets.remove(index));
                self.doc.external_stylesheets.push((*id, core::mem::take(&mut completion.reply.body)));
                if !self.doc.relayout_from_arena(fonts, width, height) {
                    self.doc.external_stylesheets.pop();
                    if let Some(previous) = previous { self.doc.external_stylesheets.insert(index.unwrap(), previous); }
                    return Err(self.doc.last_error.take().unwrap_or_else(|| "Dynamic stylesheet layout refused".into()));
                }
                self.browser.layout_dirty = true;
                Ok(true)
            }
            BrowserResourceKind::Image => {
                let decoded = crate::budget::image_bytes(&completion.bytes)?;
                let index = self.doc.nodes.iter().position(|node| matches!(node,DocNode::DomImage { node_id,.. } if node_id == id));
                let replaced_bytes = index.map_or(0, |index| match &self.doc.nodes[index] { DocNode::DomImage { pixels,.. } => pixels.len(), _ => 0 });
                let (count, bytes) = self.doc.nodes.iter().filter(|node| matches!(node, DocNode::Image { .. } | DocNode::DomImage { .. })).fold((0usize,0usize), |(count,bytes),node| {
                    let pixels = match node { DocNode::Image { pixels,.. } | DocNode::DomImage { pixels,.. } => pixels.len(), _ => 0 };
                    (count+1,bytes.saturating_add(pixels))
                });
                if count + usize::from(index.is_none()) > budget::MAX_IMAGES
                    || bytes.saturating_sub(replaced_bytes).saturating_add(decoded) > budget::MAX_IMAGE_TOTAL_BYTES
                    || self.doc.resident_bytes().saturating_add(decoded).saturating_add(completion.bytes.capacity()) > memory_limit {
                    return Err("Dynamic image exceeds document memory budget".into());
                }
                let image = crate::image::decode(&completion.bytes).ok_or("Dynamic image cannot be decoded")?;
                let previous = index.map(|index| self.doc.nodes.remove(index));
                self.doc.nodes.push(DocNode::DomImage { node_id: *id, width: image.width, height: image.height, pixels: alloc::rc::Rc::new(image.pixels) });
                if !self.doc.relayout_from_arena(fonts, width, height) {
                    self.doc.nodes.pop();
                    if let Some(previous) = previous { self.doc.nodes.insert(index.unwrap(), previous); }
                    return Err(self.doc.last_error.take().unwrap_or_else(|| "Dynamic image layout refused".into()));
                }
                self.browser.layout_dirty = true;
                Ok(true)
            }
        }
    }
}

impl OcelViewer {
    pub(crate) fn poll_browser(&mut self) {
        let active = self.active_tab;
        let tab = &mut self.tabs[active];
        if !tab.scripts_enabled || tab.doc.is_reader_mode() || tab.js_context_id == 0
            || tab.js_context_id != self.js_runtime.context_id() || !self.js_runtime.is_active() {
            tab.browser.cancel();
            return;
        }
        let width = self.width.saturating_sub(16);
        let height = self.viewport_height();
        self.reserve_active_document();
        let others: usize = self.tabs.iter().enumerate().filter(|(index,_)| *index != active).map(|(_,tab)| tab.resident_bytes()).sum();
        let tab = &mut self.tabs[active];
        let non_session = tab.resident_bytes().saturating_sub(tab.browser.session.resident_bytes());
        let available = budget::MAX_RESIDENT_BYTES.saturating_sub(others).saturating_sub(non_session);
        tab.browser.session.set_memory_budget(available);
        let result = (|| -> Result<bool,String> {
            let mut changed = tab.drain_browser_output(&mut self.js_runtime, &mut self.fonts, width, height)?;
            if tab.native_dom_dirty {
                if let Some(arena) = &tab.doc.arena { self.js_runtime.sync_document(arena, &tab.title).map_err(|error| error.message)?; }
                tab.native_dom_dirty = false;
                tab.browser.layout_dirty = true;
            }
            // Bound callback chains independently of the JS interpreter budget.
            for _ in 0..8 {
                let reply = if let Some(reply) = tab.browser.replies.pop_front() { Some(reply) }
                    else if let Some(mut completion) = tab.browser.session.poll() {
                        let retained = tab.resident_bytes().saturating_sub(tab.doc.resident_bytes());
                        let transient = completion.reply.body.capacity() + completion.reply.url.capacity()
                            + completion.reply.headers.capacity() * core::mem::size_of::<(String,String)>()
                            + completion.reply.headers.iter().map(|(key,value)| key.capacity()+value.capacity()).sum::<usize>();
                        let document_budget = budget::MAX_RESIDENT_BYTES.saturating_sub(others).saturating_sub(retained).saturating_sub(transient);
                        tab.doc.set_memory_budget(document_budget.saturating_sub(completion.bytes.capacity()));
                        if let Err(error) = tab.install_browser_resource(&mut completion, &mut self.fonts, width, height, document_budget) {
                            completion.reply.error = Some(error); completion.reply.body.clear();
                        } else if matches!(completion.request, BrowserRequest::Load { kind: BrowserResourceKind::Image | BrowserResourceKind::Stylesheet, .. }) && completion.reply.error.is_none() { changed = true; }
                        Some(completion.reply)
                    } else { None };
                let Some(reply) = reply else { break; };
                if let Some(error) = &reply.error { ostd::io::println(&format!("[ocel] browser resource: {error}")); }
                if let Err(error) = self.js_runtime.complete_browser_with_layout(&reply, &mut |mutations,node| {
                    tab.native_browser_layout(mutations,node,&mut self.fonts,width,height,true)
                }) { ostd::io::println(&format!("[ocel] browser callback: {}",error.message)); }
                changed |= tab.drain_browser_output(&mut self.js_runtime, &mut self.fonts, width, height)?;
                tab.browser.frame_pending = true;
            }
            let now = ostd::syscall::sys_get_time_ms().ok_or("Browser monotonic clock unavailable")?;
            if tab.browser.layout_dirty || tab.browser.frame_pending || self.js_runtime.next_browser_wakeup().is_some_and(|deadline| deadline <= now) {
                if let Err(error) = tab.pump_browser_frame(&mut self.js_runtime, &mut self.fonts, width, height, true) { ostd::io::println(&format!("[ocel] browser event: {error}")); }
                changed |= tab.drain_browser_output(&mut self.js_runtime, &mut self.fonts, width, height)?;
            }
            Ok(changed)
        })();
        match result {
            Ok(true) => { if self.search_open { self.perform_search(); } self.render(); }
            Ok(false) => {}
            Err(error) => {
                self.tabs[active].browser.cancel();
                self.js_runtime.reset(); self.tabs[active].js_context_id = 0;
                self.status_text = format!("Browser update rejected: {error}");
                ostd::io::println(&format!("[ocel] {}",self.status_text)); self.render_chrome();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tab(html: &str, fonts: &mut TextFonts) -> Tab {
        let parsed = parser::html::parse_html(html);
        let mut doc = Document::new();
        doc.nodes = parsed.nodes;
        doc.arena = Some(parsed.arena);
        assert!(doc.relayout_from_arena(fonts, 600, 400));
        Tab {
            title: String::new(), url: "https://example.test/page".into(), doc, scroll_y: 0,
            history: Vec::new(), history_idx: 0, js_context_id: 0, scripts_enabled: true,
            resources: resources::ResourceBundle::default(), native_dom_dirty: false,
            browser: DocumentBrowser::new("https://example.test/page"),
        }
    }
    fn id(tab: &Tab, name: &str) -> dom_arena::NodeId {
        tab.doc.arena.as_ref().unwrap().nodes.iter().find(|node| node.get_attribute("id") == Some(name)).unwrap().id
    }
    fn width(tab: &Tab, node: dom_arena::NodeId) -> u32 {
        tab.doc.web_layout.as_ref().unwrap().boxes.iter().find(|b| b.node_id == Some(node)).unwrap().width
    }
    fn stylesheet(node: dom_arena::NodeId, body: String) -> browser::BrowserCompletion {
        browser::BrowserCompletion {
            request: BrowserRequest::Load { id: 1, node: Some(node), url: "/live.css".into(), kind: BrowserResourceKind::Stylesheet },
            reply: BrowserReply { id: 1, url: "https://example.test/live.css".into(), status: 200, headers: Vec::new(), body, error: None },
            bytes: Vec::new(),
        }
    }
    #[test]
    fn dynamic_css_changes_native_geometry_and_stale_or_excess_css_cannot_replace_it() {
        let mut fonts = TextFonts::new();
        let mut tab = tab("<style>#target{width:100px;height:20px}</style><link id='theme' rel='stylesheet' href='/live.css'><div id='target'>Content</div>", &mut fonts);
        let target = id(&tab, "target");
        let link = id(&tab, "theme");
        assert_eq!(width(&tab, target), 100);
        tab.install_browser_resource(&mut stylesheet(link, "#target{width:220px}".into()), &mut fonts, 600, 400, budget::MAX_RESIDENT_BYTES).unwrap();
        assert_eq!(width(&tab, target), 220);
        let excess = "#target{width:999px}".repeat(budget::MAX_CSS_RULES + 1);
        assert!(tab.install_browser_resource(&mut stylesheet(link, excess), &mut fonts, 600, 400, budget::MAX_RESIDENT_BYTES).is_err());
        assert_eq!(width(&tab, target), 220);
        assert!(tab.doc.arena.as_mut().unwrap().apply_mutation(&dom_arena::DomMutation::SetAttribute { node: link, key: "href".into(), val: "/new.css".into() }));
        assert!(tab.install_browser_resource(&mut stylesheet(link, "#target{width:999px}".into()), &mut fonts, 600, 400, budget::MAX_RESIDENT_BYTES).is_err());
        assert_eq!(width(&tab, target), 220);
    }

    #[test]
    fn detached_image_preload_is_available_when_the_node_is_attached() {
        let mut fonts = TextFonts::new();
        let mut tab = tab("<body><div id='parent'></div></body>", &mut fonts);
        let parent = id(&tab, "parent");
        let arena = tab.doc.arena.as_mut().unwrap();
        let image = arena.alloc_node(dom_arena::NodeData::Element { tag: "img".into(), attributes: alloc::vec![("src".into(), "/pixel.bmp".into())] });
        let mut bmp = alloc::vec![0u8; 58];
        bmp[..2].copy_from_slice(b"BM");
        bmp[2..6].copy_from_slice(&58u32.to_le_bytes());
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&1i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&1i32.to_le_bytes());
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
        bmp[54..57].copy_from_slice(&[30, 20, 10]);
        let mut completion = browser::BrowserCompletion {
            request: BrowserRequest::Load { id: 2, node: Some(image), url: "/pixel.bmp".into(), kind: BrowserResourceKind::Image },
            reply: BrowserReply { id: 2, url: "https://example.test/pixel.bmp".into(), status: 200, headers: Vec::new(), body: String::new(), error: None },
            bytes: bmp,
        };
        tab.install_browser_resource(&mut completion, &mut fonts, 600, 400, budget::MAX_RESIDENT_BYTES).unwrap();
        assert!(tab.doc.arena.as_mut().unwrap().append_child(parent, image));
        assert!(tab.doc.relayout_from_arena(&mut fonts, 600, 400));
        let image_box = tab.doc.web_layout.as_ref().unwrap().boxes.iter().find(|b| b.node_id == Some(image)).unwrap();
        let (width, height, pixels) = image_box.image.as_ref().unwrap();
        assert_eq!((*width, *height), (1, 1));
        assert_eq!(&pixels[..4], &[30, 20, 10, 255]);
    }
    #[test]
    fn native_cssom_preserves_inline_ancestor_geometry_and_content_box_dimensions() {
        let mut fonts = TextFonts::new();
        let tab = tab("<div id='container' style='position:relative;width:100px;padding:10px'><a id='outer'><b id='inner'>Buy</b></a></div>", &mut fonts);
        let arena = tab.doc.arena.as_ref().unwrap();
        let web = tab.doc.web_layout.as_ref().unwrap();
        let outer = web.browser_node_layout(arena,&[],id(&tab,"outer"),600,400,0).unwrap();
        let inner = web.browser_node_layout(arena,&[],id(&tab,"inner"),600,400,0).unwrap();
        assert_eq!((outer.x,outer.y,outer.width,outer.height),(inner.x,inner.y,inner.width,inner.height));
        assert!(outer.width > 0.0);
        let container = web.browser_node_layout(arena,&[],id(&tab,"container"),600,400,0).unwrap();
        assert_eq!(container.width,120.0);
        let property = |name:&str| container.styles.iter().find(|(key,_)|key==name).unwrap().1.as_str();
        assert_eq!(property("width"),"100px");
        assert_eq!(property("position"),"relative");
        assert_eq!(property("padding-left"),"10px");
    }
}
