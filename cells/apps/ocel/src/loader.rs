// SPDX-License-Identifier: MIT
//! Bounded document acquisition and cooperative network/resource loading.
extern crate alloc;
use alloc::{format, string::String, vec::Vec};
use core::task::Poll;
use crate::{doc::DocNode, resources::{ResourceBundle, Reference}};

pub struct LoadedDocument {
    pub url: String,
    pub title: String,
    pub content: String,
    pub direct_nodes: Option<Vec<DocNode>>,
    pub resources: ResourceBundle,
    /// Native-only response metadata, including HttpOnly cookies.
    pub response_headers: Vec<(String, String)>,
}
impl LoadedDocument {
    fn text(url: &str, title: String, content: String) -> Self {
        Self { url: url.into(), title, content, direct_nodes: None, resources: ResourceBundle::default(), response_headers: Vec::new() }
    }
    fn resident_bytes(&self) -> usize {
        self.url.capacity() + self.title.capacity() + self.content.capacity() + self.resources.resident_bytes()
            + self.response_headers.capacity() * core::mem::size_of::<(String, String)>()
            + self.response_headers.iter().map(|(key,value)| key.capacity()+value.capacity()).sum::<usize>()
            + self.direct_nodes.as_ref().map(|nodes| nodes.capacity() * core::mem::size_of::<DocNode>()
                + nodes.iter().map(|node| match node { DocNode::Image { pixels, .. } => pixels.capacity(), _ => 256 }).sum::<usize>()).unwrap_or(0)
    }
}

pub fn poll_cleanup() { crate::net::http::poll_cleanup(); }

/// A navigation owns its exact operations. Dropping/cancelling cannot publish a
/// completion to another navigation, even when the provider replies late.
struct ActiveResourceFetch {
    url: String,
    image: bool,
    job: crate::net::http::FetchJob,
}

pub const MAX_CONCURRENT_RESOURCE_FETCHES: usize = crate::budget::MAX_CONCURRENT_RESOURCE_FETCHES;

pub struct DocumentLoadJob {
    url: String,
    network: Option<crate::net::http::FetchJob>,
    active_fetches: Vec<ActiveResourceFetch>,
    loaded: Option<LoadedDocument>,
    references: Vec<Reference>,
    next_resource: usize,
    completed_resources: usize,
    deadline: u64,
    progress: String,
    finished: bool,
    memory_budget: usize,
    reader_mode: bool,
    early_document_taken: bool,
}
impl DocumentLoadJob {
    pub fn new(url: &str) -> Result<Self, String> {
        let url = url.trim();
        if url.is_empty() || url.len() > 2048 { return Err("Empty or overlong document URL".into()); }
        let network = if is_network(url) {
            let path = url.split(|c| c == '?' || c == '#').next().unwrap_or(url);
            if crate::pdf::is_pdf_path(path) { return Err("Network PDF viewing is unsupported; open a local PDF file".into()); }
            let limit = if crate::image::is_image_path(path) { crate::budget::MAX_ENCODED_IMAGE_BYTES } else { crate::budget::MAX_TEXT_BYTES };
            Some(crate::net::http::FetchJob::new(url, limit)?)
        } else {
            if url.contains("://") && !url.starts_with("file://") && !matches!(url, "ocel://welcome" | "ocel://help" | "ocel://bookmarks") {
                return Err("Unsupported document URL scheme".into());
            }
            None
        };
            let now = ostd::syscall::sys_get_time_ms().unwrap_or(0);
            let deadline = now.saturating_add(60_000);
            Ok(Self { url: url.into(), network, active_fetches: Vec::new(), loaded: None, references: Vec::new(),
                next_resource: 0, completed_resources: 0,
                deadline, progress: "Opening document".into(), finished: false, memory_budget: crate::budget::MAX_RESIDENT_BYTES,
                reader_mode: false, early_document_taken: false })
    }
    pub fn set_reader_mode(&mut self, reader: bool) { self.reader_mode = reader; }
    pub fn is_reader_mode(&self) -> bool { self.reader_mode }
    pub fn take_early_document(&mut self) -> Option<LoadedDocument> {
        if self.early_document_taken || self.finished {
            return None;
        }
        let loaded = self.loaded.as_ref()?;
        if !matches!(crate::parser::DocFormat::detect_from_url_or_content(&loaded.url, &loaded.content),
            crate::parser::DocFormat::Html | crate::parser::DocFormat::Markdown)
        {
            return None;
        }
        self.early_document_taken = true;
        Some(LoadedDocument {
            url: loaded.url.clone(),
            title: loaded.title.clone(),
            content: loaded.content.clone(),
            direct_nodes: None,
            resources: ResourceBundle::default(),
            response_headers: loaded.response_headers.clone(),
        })
    }
    pub fn resident_bytes(&self) -> usize {
        core::mem::size_of::<Self>() + self.url.capacity() + self.progress.capacity()
            + self.references.capacity() * core::mem::size_of::<Reference>()
            + self.references.iter().map(|reference| reference.url.capacity()).sum::<usize>()
            + self.loaded.as_ref().map(LoadedDocument::resident_bytes).unwrap_or(0)
            + self.network.as_ref().map(crate::net::http::FetchJob::resident_bytes).unwrap_or(0)
            + self.active_fetches.iter().map(|a| a.job.resident_bytes() + a.url.capacity()).sum::<usize>()
    }
    pub fn set_memory_budget(&mut self, bytes: usize) { self.memory_budget = bytes.min(crate::budget::MAX_RESIDENT_BYTES); }
    pub fn progress(&self) -> &str {
        if self.loaded.is_none() { if let Some(network) = &self.network { return network.progress(); } }
        &self.progress
    }
    pub fn cancel(&mut self) {
        if let Some(mut network) = self.network.take() { network.cancel(); }
        for mut active in core::mem::take(&mut self.active_fetches) {
            active.job.cancel();
        }
        self.loaded = None; self.references.clear(); self.finished = true;
        self.progress = "Cancelled".into();
    }
    fn fail(&mut self, error: String) -> Poll<Result<LoadedDocument, String>> { self.cancel(); Poll::Ready(Err(error)) }
    pub fn poll(&mut self) -> Poll<Result<LoadedDocument, String>> {
        poll_cleanup();
        if self.finished { return Poll::Pending; }
        if let Some(now) = ostd::syscall::sys_get_time_ms() {
            if now >= self.deadline {
                return self.fail("Document/resource loading deadline exceeded".into());
            }
        }
        let resident = self.resident_bytes();
        if resident.saturating_add(64 * 1024) > self.memory_budget { return self.fail("Document acquisition exceeds remaining tab memory budget".into()); }
        let available = self.memory_budget.saturating_sub(resident + 64 * 1024);
        if let Some(network) = &mut self.network {
            network.set_memory_budget(available.saturating_add(network.resident_bytes()));
        }
        if self.loaded.is_none() {
            let result = if let Some(network) = &mut self.network {
                match network.poll() {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(error)) => return self.fail(error),
                    Poll::Ready(Ok(result)) => network_document(result, available),
                }
            } else { load_local(&self.url, available) };
            self.network = None;
            let loaded = match result { Ok(loaded) => loaded, Err(error) => return self.fail(error) };
            if loaded.content.len().saturating_mul(12).saturating_add(128 * 1024) > available {
                return self.fail("Document resource discovery exceeds remaining tab memory budget".into());
            }
            self.references = if loaded.direct_nodes.is_none() && matches!(crate::parser::DocFormat::detect_from_url_or_content(&loaded.url, &loaded.content), crate::parser::DocFormat::Html | crate::parser::DocFormat::Markdown) {
                match crate::resources::references(&loaded.url, &loaded.content) {
                    Ok(mut references) => {
                        if self.reader_mode {
                            references.retain(|r| r.image);
                        }
                        references
                    }
                    Err(error) => {
                        if self.reader_mode {
                            ostd::io::println(&alloc::format!("[ocel] reader mode: ignoring raw HTML reference discovery warning: {}", error));
                            Vec::new()
                        } else {
                            return self.fail(error);
                        }
                    }
                }
            } else { Vec::new() };
            self.loaded = Some(loaded);
            return Poll::Pending;
        }
        let loaded = self.loaded.as_mut().unwrap();

        // 1. Poll in-flight parallel downloads:
        let mut idx = 0;
        while idx < self.active_fetches.len() {
            match self.active_fetches[idx].job.poll() {
                Poll::Pending => {
                    idx += 1;
                }
                Poll::Ready(result) => {
                    let active = self.active_fetches.remove(idx);
                    self.completed_resources += 1;
                    let body_result = match result {
                        Ok(res) => {
                            ostd::io::println(&alloc::format!("[ocel] resource fetched: {} ({} bytes)", active.url, res.body.len()));
                            Ok(res.body)
                        }
                        Err(err) => {
                            ostd::io::println(&alloc::format!("[ocel] resource fetch failed: {} ({})", active.url, err));
                            Err(err)
                        }
                    };
                    if let Err(error) = loaded.resources.insert(active.url, body_result, active.image) {
                        return self.fail(error);
                    }
                }
            }
        }

        // 2. Replenish worker pool up to MAX_CONCURRENT_RESOURCE_FETCHES:
        while self.active_fetches.len() < MAX_CONCURRENT_RESOURCE_FETCHES && self.next_resource < self.references.len() {
            let reference = &self.references[self.next_resource];
            self.next_resource += 1;
            let limit = (if reference.image { crate::budget::MAX_ENCODED_IMAGE_BYTES } else { crate::budget::MAX_CSS_BYTES })
                .min(loaded.resources.remaining_bytes()).min(available);
            let resource_url = &reference.url;
            if limit == 0 { return self.fail("Document resource budget exhausted".into()); }
            if is_network(resource_url) {
                match crate::net::http::FetchJob::same_site(resource_url, limit, &loaded.url) {
                    Ok(job) => {
                        self.active_fetches.push(ActiveResourceFetch {
                            url: resource_url.clone(),
                            image: reference.image,
                            job,
                        });
                    }
                    Err(error) => {
                        ostd::io::println(&alloc::format!("[ocel] resource same_site rejected: {} ({})", resource_url, error));
                        if let Err(error) = loaded.resources.insert(resource_url.clone(), Err(error), reference.image) {
                            return self.fail(error);
                        }
                        self.completed_resources += 1;
                    }
                }
            } else {
                let result = crate::resources::read_local(resource_url, limit);
                if let Err(error) = loaded.resources.insert(resource_url.clone(), result, reference.image) {
                    return self.fail(error);
                }
                self.completed_resources += 1;
            }
        }

        // 3. Progress report:
        if self.references.is_empty() {
            self.progress = "Loaded".into();
        } else {
            self.progress = format!("Loading resources ({}/{})...", self.completed_resources.min(self.references.len()), self.references.len());
        }

        // 4. Check completion:
        if self.active_fetches.is_empty() && self.next_resource >= self.references.len() {
            self.finished = true;
            self.progress = "Loaded".into();
            return Poll::Ready(Ok(self.loaded.take().unwrap()));
        }

        Poll::Pending
    }
}
impl Drop for DocumentLoadJob { fn drop(&mut self) { self.cancel(); } }
fn is_network(url: &str) -> bool { url.starts_with("http://") || url.starts_with("https://") || url.starts_with("gemini://") }

fn network_document(result: crate::net::http::FetchResult, memory_budget: usize) -> Result<LoadedDocument, String> {
    let path = result.url.split(|c| c == '?' || c == '#').next().unwrap_or(&result.url);
    let media_type = result.media_type.split(';').next().unwrap_or("").trim();
    if media_type.eq_ignore_ascii_case("application/pdf") {
        return Err("Network PDF viewing is unsupported; open a local PDF file".into());
    }
    if crate::image::is_image_path(path) || media_type.split_once('/').is_some_and(|(major, subtype)| major.eq_ignore_ascii_case("image") && !subtype.is_empty()) {
        let mut loaded = image_document(&result.url, &result.body, memory_budget)?;
        loaded.response_headers = result.headers;
        return Ok(loaded);
    }
    let content = String::from_utf8(result.body).map_err(|_| String::from("Network document is not UTF-8"))?;
    crate::budget::validate_text(&content)?;
    let mut loaded = LoadedDocument::text(&result.url, format!("Ocel — {}", result.url), content);
    loaded.response_headers = result.headers;
    if result.gemini && media_type.eq_ignore_ascii_case("text/gemini") {
        let lines = loaded.content.lines().count();
        let reserve = loaded.content.len().saturating_mul(8)
            .saturating_add(lines.saturating_mul(core::mem::size_of::<DocNode>() * 4));
        if lines > crate::budget::MAX_DOM_NODES || reserve > memory_budget {
            return Err("Gemtext exceeds document node/memory budget".into());
        }
        loaded.direct_nodes = Some(crate::net::parse_gemtext(&loaded.content, &loaded.url));
    }
    Ok(loaded)
}

fn image_document(url: &str, bytes: &[u8], memory_budget: usize) -> Result<LoadedDocument, String> {
    if bytes.len() > crate::budget::MAX_ENCODED_IMAGE_BYTES { return Err("Image exceeds encoded byte budget".into()); }
    if crate::budget::image_bytes(bytes)? > memory_budget { return Err("Image decoding exceeds remaining tab memory budget".into()); }
    let image = crate::image::decode(bytes).ok_or("Unsupported, corrupt, or oversized image (PNG/JPEG/BMP only)")?;
    let mut loaded = LoadedDocument::text(url, format!("Ocel — Image ({})", url), String::new());
    loaded.direct_nodes = Some(alloc::vec![DocNode::Image { width: image.width, height: image.height, pixels: alloc::rc::Rc::new(image.pixels) }]);
    Ok(loaded)
}

fn load_local(url: &str, memory_budget: usize) -> Result<LoadedDocument, String> {
    let suffixless = url.split(|c| c == '?' || c == '#').next().unwrap_or(url);
    let path = suffixless.strip_prefix("file://").unwrap_or(suffixless);
    if suffixless == "ocel://welcome" || matches!(path, "welcome" | "/welcome" | "/welcome.md" | "welcome.md") {
        return Ok(LoadedDocument::text("file:///welcome.md", "Ocel — Welcome Guide".into(), get_welcome_content()));
    }
    if suffixless == "ocel://help" || matches!(path, "help" | "/help" | "/help.md" | "help.md") {
        return Ok(LoadedDocument::text("file:///help.md", "Ocel — Keyboard & Usage Help".into(), get_help_content()));
    }
    if suffixless == "ocel://bookmarks" || matches!(path, "bookmarks" | "/bookmarks" | "/bookmarks.md" | "bookmarks.md") {
        return Ok(LoadedDocument::text("ocel://bookmarks", "Ocel — Bookmarks".into(), get_bookmarks_content()));
    }
    let path = crate::resources::local_path(url)?;
    if crate::pdf::is_pdf_path(&path) {
        if memory_budget < crate::budget::MAX_IMAGE_TOTAL_BYTES { return Err("PDF raster exceeds remaining tab memory budget".into()); }
        let page = crate::pdf::page_number(url).and_then(|page| crate::pdf::load_page(&path, page))?;
        let mut loaded = LoadedDocument::text(url, format!("Ocel — PDF ({})", path), String::new());
        loaded.direct_nodes = Some(page.into_nodes(url)); return Ok(loaded);
    }
    if crate::image::is_image_path(&path) {
        let bytes = crate::resources::read_local(url, crate::budget::MAX_ENCODED_IMAGE_BYTES.min(memory_budget / 2))?;
        return image_document(url, &bytes, memory_budget.saturating_sub(bytes.capacity()));
    }
    let content = String::from_utf8(crate::resources::read_local(url, crate::budget::MAX_TEXT_BYTES.min(memory_budget / 12))?).map_err(|_| String::from("Local document is not UTF-8"))?;
    crate::budget::validate_text(&content)?;
    Ok(LoadedDocument::text(url, format!("Ocel — {}", path.rsplit('/').next().unwrap_or(&path)), content))
}

/// Local-only convenience for built-ins/tests. Network navigation must use a job.
pub fn load_document(url: &str) -> LoadedDocument {
    let result = if is_network(url) { Err(String::from("Network navigation requires DocumentLoadJob")) } else { load_local(url.trim(), crate::budget::MAX_RESIDENT_BYTES) };
    result.unwrap_or_else(|error| LoadedDocument::text(url, "Ocel — Load Error".into(), format!("# Document Load Error\n\nCould not open `{}`.\n\n{}", url, error)))
}

fn get_welcome_content() -> String {
    format!(
        r#"# Welcome to Ocel

**Ocel** (derived from *Ocellus* — the simple eye) is the native document and web content viewer for **CellOS**.

---

### Key Capabilities
- **Native Rendering:** Documents and images use the CellOS display pipeline without a guest OS.
- **Native Viewing:** Markdown, plain text, source code, an HTML subset, PNG/JPEG/BMP images, and local PDFs rendered by the isolated native `ocel-pdf` service.
- **JavaScript Engine:** Images that package the Tier 2 QuickJS cell execute real JavaScript. Browser DOM, CSS and Web API compatibility remain under development.

### Tiếng Việt & Đa ngôn ngữ
Hỗ trợ đầy đủ bảng chữ cái tiếng Việt có dấu:
- **Nguyên âm:** á, à, ả, ã, ạ, ă, ắ, ằ, ẳ, ẵ, ặ, â, ấ, ầ, ẩ, ẫ, ậ...
- **Phụ âm đặc biệt:** đ, Đ
- **Đọc tài liệu:** Hiển thị trơn tru mọi tài liệu Markdown, HTML và văn bản tiếng Việt!


### So sánh Tính năng (Feature Matrix)

| Tiêu chí | Ocel (Tier 2) | Chrome (Tier 3) |
| -------- | ------------- | --------------- |
| Bộ nhớ | {} profile: {} MiB heap / {} MiB shared resident budget; display and engine allocations are additional | Depends on guest and workload |
| Định dạng | MD, HTML subset, code, PNG/JPEG/BMP, local PDF | Full Web Apps, SPA |
| Bảo mật | Hardware MMU | Stage-2 Hypervisor |
---

### Quick Navigation
- Type `file:///help.md` in the address bar above to see shortcuts.
- Use `Up` / `Down` arrows or `PageUp` / `PageDown` to scroll.
- Press `Enter` to navigate or reload.

```rust
// Ocel Cell Architecture
// Running in CellOS SAS / Tier 2 Native Domain
fn main() {{
    println!("Hello from Ocel!");
}}
```

*Built with passion for CellOS.*
"#,
        crate::budget::PROFILE_NAME, crate::budget::HEAP_BYTES / (1024 * 1024),
        crate::budget::MAX_RESIDENT_BYTES / (1024 * 1024),
    )
}

fn get_help_content() -> String {
    format!(
        r#"# Ocel Keyboard & Navigation Help

### Keyboard Shortcuts
- **Up Arrow / Down Arrow:** Scroll viewport by 1 line (24 px).
- **Space / Shift+Space:** Scroll down / up by one screen height.
- **PageUp / PageDown:** Scroll viewport by one screen height.
- **Home / End:** Jump to top or bottom of document.
- **Enter:** Navigate to URL typed in the address bar.
- **Ctrl+L / F6:** Focus and edit the address bar.
- **F5 / Escape:** Reload / cancel an in-progress request or dismiss dialogs/image zoom.
- **Ctrl+R:** Toggle Reader mode for this tab (original document is preserved).
- **Alt+T:** Cycle Reader theme (Follow System / Light / Dark / Paper).
- **Alt+[ / Alt+]:** Narrow / widen the Reader column.
- **Ctrl+plus / Ctrl+minus / Ctrl+0:** Zoom in / out / reset (50–200%).
- **Ctrl+M:** Heading contents; choose with arrows, wheel, Enter or a click.
- **Ctrl+F / F3 / Shift+F3:** Find / next match / previous match.
- **Ctrl+D / Ctrl+B:** Save bookmark to `/data/bookmarks.md` / Open bookmarks.
- **Ctrl+S:** Export cleaned article to Markdown (`/data/saved-*.md`).
- **Image Zoom:** Click any image in Reader mode to zoom fullscreen; Escape or click to dismiss.
---

### Supported URL Schemes
- `file:///path/to/file.md` — Open Markdown file from VFS.
- `file:///path/to/file.txt` — Open plain text or source code.
- `file:///path/to/file.html` — Open HTML document.
- `file:///path/to/file.pdf#page=2` — View a local PDF page (one-based; no fragment opens page 1).
- **PDF navigation:** Click Previous page / Next page above the raster. PageUp / PageDown still scroll; tab shortcuts are unchanged.
- **PDF availability:** Requires the registered native `ocel-pdf` service. One opaque page at a time, bounded to 1024 × 1024 pixels; the service accepts PDFs up to 2 MiB. No fallback renderer or PDF-source text view.
- `http://`, `https://`, `gemini://` — Bounded cooperative network navigation; Escape cancels DNS, connect or reading. HTTPS/Gemini require the net service's certificate and authenticated-time policy.
- **Resource policy:** Only same-origin static images, classic scripts and stylesheets are prefetched, within the shared resource budget. Other protocols and network PDF viewing are explicitly unsupported.
- **Build profile:** {} — {} MiB viewer heap, {} MiB shared tab/acquisition residency, {} MiB document text and {} MiB aggregate resources. This changes bounded capacities, not browser compatibility.
- `file:///welcome.md` — Built-in Welcome guide.
- `file:///help.md` — This help page.
"#,
        crate::budget::PROFILE_NAME, crate::budget::HEAP_BYTES / (1024 * 1024),
        crate::budget::MAX_RESIDENT_BYTES / (1024 * 1024), crate::budget::MAX_TEXT_BYTES / (1024 * 1024),
        crate::budget::MAX_RESOURCE_BYTES / (1024 * 1024),
    )
}

fn get_bookmarks_content() -> String {
    if let Ok(mut file) = ostd::fs::File::open("/data/bookmarks.md") {
        let mut bytes = Vec::new(); let mut chunk = [0u8; 4096];
        loop {
            match file.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => {
                    if count > crate::budget::MAX_TEXT_BYTES.saturating_sub(bytes.len()) { return "# Bookmarks Error\n\nBookmarks file exceeds text byte budget.".into(); }
                    bytes.extend_from_slice(&chunk[..count]);
                }
                Err(_) => return "# Bookmarks Error\n\nCould not read the bookmarks file.".into(),
            }
        }
        match String::from_utf8(bytes) {
            Ok(content) if !content.trim().is_empty() => return content,
            Ok(_) => {},
            Err(_) => return "# Bookmarks Error\n\nBookmarks file is not UTF-8.".into(),
        }
    }
    String::from(
        r#"# 📑 Ocel Bookmarks & Quick Links

Danh sách liên kết nhanh và cổng thông tin được chọn lọc cho CellOS:

---

### 🌐 Tra Cứu & Tìm Kiếm Trực Tuyến
- [🦆 DuckDuckGo Lite](https://html.duckduckgo.com/html/) — Máy tìm kiếm văn bản siêu nhẹ, bảo vệ quyền riêng tư, không JavaScript.
- [📚 Wikipedia Mobile](https://en.m.wikipedia.org) — Bách khoa toàn thư mở phiên bản di động gọn nhẹ.
- [📰 Hacker News](https://news.ycombinator.com) — Diễn đàn tin tức công nghệ và lập trình.
- [🦀 Rust Standard Library](https://doc.rust-lang.org/std/) — Tra cứu tài liệu thư viện chuẩn Rust.

---

### 💻 Trang Cục Bộ & Quản Trị Hệ Thống
- [🏠 Trang Chào Mừng Ocel](file:///welcome.md) — Giới thiệu kiến trúc Ocel và tính năng.
- [❓ Trợ Giúp Phím Tắt](file:///help.md) — Danh sách phím tắt và mẹo duyệt web.
- [⚡ C2C Gateway API](http://10.0.2.2:8080/api/status) — Cổng HTTP Async C2C Gateway nội bộ CellOS.

---
*Nhấn `Ctrl+B` để mở Bookmarks. Hiện chưa hỗ trợ lưu dấu trang bằng `Ctrl+D`; có thể chỉnh sửa `/data/bookmarks.md` bên ngoài Ocel.*
"#,
    )
}

pub fn classify_smart_url(input: &str) -> String {
    let s = input.trim();
    if s.is_empty() {
        return String::from("file:///welcome.md");
    }

    // Preserve explicit schemes so acquisition can reject unsupported protocols;
    // never silently reinterpret one as a supported HTTPS address.
    if s.contains("://") {
        return String::from(s);
    }

    if s == "welcome" || s == "help" || s == "bookmarks" {
        return format!("ocel://{}", s);
    }

    if s.starts_with('/') {
        return format!("file://{}", s);
    }

    let has_spaces = s.contains(char::is_whitespace);
    let has_dot = s.contains('.');
    let is_localhost = s.starts_with("localhost") || s.starts_with("127.0.0.1") || s.starts_with("10.0.2.2");

    if !has_spaces && (has_dot || is_localhost) {
        return format!("https://{}", s);
    }

    let mut query_encoded = String::new();
    for ch in s.chars() {
        match ch {
            ' ' => query_encoded.push('+'),
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => query_encoded.push(ch),
            _ => {
                let mut buf = [0u8; 4];
                for b in ch.encode_utf8(&mut buf).bytes() {
                    let _ = alloc::fmt::write(&mut query_encoded, format_args!("%{:02X}", b));
                }
            }
        }
    }
    format!("https://html.duckduckgo.com/html/?q={}", query_encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_smart_url() {
        assert_eq!(classify_smart_url(""), "file:///welcome.md");
        assert_eq!(classify_smart_url("   "), "file:///welcome.md");
        assert_eq!(classify_smart_url("welcome"), "ocel://welcome");
        assert_eq!(classify_smart_url("bookmarks"), "ocel://bookmarks");
        assert_eq!(classify_smart_url("https://cellos.org"), "https://cellos.org");
        assert_eq!(classify_smart_url("http://example.com/foo"), "http://example.com/foo");
        assert_eq!(classify_smart_url("file:///data/demo.html"), "file:///data/demo.html");
        assert_eq!(classify_smart_url("/etc/hosts"), "file:///etc/hosts");
        assert_eq!(classify_smart_url("news.ycombinator.com"), "https://news.ycombinator.com");
        assert_eq!(classify_smart_url("wikipedia.org/wiki/Rust"), "https://wikipedia.org/wiki/Rust");
        assert_eq!(classify_smart_url("localhost:8080/api"), "https://localhost:8080/api");
        assert_eq!(
            classify_smart_url("rust microkernel"),
            "https://html.duckduckgo.com/html/?q=rust+microkernel"
        );
    }

    #[test]
    fn test_virtual_bookmarks_loading() {
        let doc = load_document("ocel://bookmarks");
        assert_eq!(doc.url, "ocel://bookmarks");
        assert_eq!(doc.title, "Ocel — Bookmarks");
        assert!(doc.content.contains("DuckDuckGo Lite"));
        assert!(doc.content.contains("Wikipedia Mobile"));
    }
    #[test]
    fn extensionless_mixed_case_image_content_type_is_decoded() {
        let mut bmp = alloc::vec![0u8; 58];
        bmp[..2].copy_from_slice(b"BM");
        bmp[2..6].copy_from_slice(&58u32.to_le_bytes());
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&1i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&1i32.to_le_bytes());
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
        // Encoded-byte admission uses the build profile even for extensionless images.
        bmp.resize(crate::budget::MAX_ENCODED_IMAGE_BYTES, 0);
        assert!(image_document("https://host/asset", &bmp, 4096).is_ok());
        bmp.push(0);
        assert!(matches!(image_document("https://host/asset", &bmp, 4096), Err(error) if error.contains("encoded byte budget")));
        bmp.truncate(58);
        let result = crate::net::http::FetchResult {
            url: "https://host/asset?id=1".into(), body: bmp,
            media_type: " Image/BMP ; profile=screen".into(), gemini: false,
            status: 200, headers: Vec::new(),
        };
        let loaded = network_document(result, 4096).unwrap();
        assert!(matches!(loaded.direct_nodes.as_ref().and_then(|nodes| nodes.first()), Some(DocNode::Image { width: 1, height: 1, .. })));
    }

    #[test]
    fn test_document_load_job_reader_mode_and_early_document() {
        let mut job = DocumentLoadJob::new("file:///welcome.md").unwrap();
        assert!(!job.is_reader_mode());
        job.set_reader_mode(true);
        assert!(job.is_reader_mode());
        assert!(job.take_early_document().is_none());
    }
}
