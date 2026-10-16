// SPDX-License-Identifier: MIT
//! Document loader for Ocel (VFS file retrieval & virtual documents).

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::doc::DocNode;

pub struct LoadedDocument {
    pub url: String,
    pub title: String,
    pub content: String,
    pub direct_nodes: Option<Vec<DocNode>>,
}

pub fn load_document(url: &str) -> LoadedDocument {
    let clean_url = url.trim();

    // Strip "file://" prefix if provided
    let path = if let Some(stripped) = clean_url.strip_prefix("file://") {
        stripped
    } else {
        clean_url
    };

    // 1. Check for built-in virtual documents
    if clean_url == "ocel://welcome" || path == "welcome" || path == "/welcome" || path == "/welcome.md" || path == "welcome.md" {
        return LoadedDocument {
            url: String::from("file:///welcome.md"),
            title: String::from("Ocel — Welcome Guide"),
            content: get_welcome_content(),
            direct_nodes: None,
        };
    }

    if clean_url == "ocel://help" || path == "help" || path == "/help" || path == "/help.md" || path == "help.md" {
        return LoadedDocument {
            url: String::from("file:///help.md"),
            title: String::from("Ocel — Keyboard & Usage Help"),
            content: get_help_content(),
            direct_nodes: None,
        };
    }

    if clean_url == "ocel://bookmarks" || path == "bookmarks" || path == "/bookmarks" || path == "/bookmarks.md" || path == "bookmarks.md" {
        return LoadedDocument {
            url: String::from("ocel://bookmarks"),
            title: String::from("Ocel — Bookmarks"),
            content: get_bookmarks_content(),
            direct_nodes: None,
        };
    }
    // 2. Check for HTTP network URL
    if clean_url.starts_with("http://") {
        return match crate::net::fetch_http(clean_url) {
            Ok(content) => {
                let display_host = clean_url
                    .trim_start_matches("http://")
                    .split('/')
                    .next()
                    .unwrap_or(clean_url);
                LoadedDocument {
                    url: String::from(clean_url),
                    title: format!("Ocel — {}", display_host),
                    content,
                    direct_nodes: None,
                }
            }
            Err(err) => LoadedDocument {
                url: String::from(clean_url),
                title: String::from("Ocel — Network Error"),
                content: format!(
                    "# Network Error\n\nCould not fetch `{}`.\n\n**Reason:** {}\n\n### Suggestions:\n- Verify host address and port.\n- Ensure CellOS `service-net` is active.\n- Try connecting to gateway: `http://10.0.2.2:8080/`",
                    clean_url, err
                ),
                direct_nodes: None,
            },
        };
    }

    // 3. Check for HTTPS encrypted network URL
    if clean_url.starts_with("https://") {
        return match crate::net::fetch_https(clean_url) {
            Ok(content) => {
                let display_host = clean_url
                    .trim_start_matches("https://")
                    .split('/')
                    .next()
                    .unwrap_or(clean_url);
                LoadedDocument {
                    url: String::from(clean_url),
                    title: format!("Ocel — {} [HTTPS]", display_host),
                    content,
                    direct_nodes: None,
                }
            }
            Err(err) => LoadedDocument {
                url: String::from(clean_url),
                title: String::from("Ocel — TLS Error"),
                content: format!(
                    "# HTTPS / TLS Connection Error\n\nCould not securely fetch `{}`.\n\n**Reason:** {}\n\n### Suggestions:\n- Check network connection.\n- Ensure CellOS `service-net` TLS broker is active.\n- Verify host and port 443.",
                    clean_url, err
                ),
                direct_nodes: None,
            },
        };
    }
    // 3.5 Check for Gemini network URL (gemini://)
    if clean_url.starts_with("gemini://") {
        return match crate::net::fetch_gemini(clean_url) {
            Ok(resp) => {
                let display_host = clean_url
                    .trim_start_matches("gemini://")
                    .split('/')
                    .next()
                    .unwrap_or(clean_url);
                if resp.status >= 20 && resp.status < 30 {
                    let nodes = crate::net::parse_gemtext(&resp.body, clean_url);
                    LoadedDocument {
                        url: String::from(clean_url),
                        title: format!("Ocel — {} [Gemini]", display_host),
                        content: String::new(),
                        direct_nodes: Some(nodes),
                    }
                } else if resp.status >= 30 && resp.status < 40 {
                    LoadedDocument {
                        url: String::from(clean_url),
                        title: format!("Ocel — Redirect ({})", resp.status),
                        content: format!(
                            "# Gemini Redirect (Status {})\n\nTarget: [{}]({})",
                            resp.status, resp.meta, resp.meta
                        ),
                        direct_nodes: None,
                    }
                } else {
                    LoadedDocument {
                        url: String::from(clean_url),
                        title: format!("Ocel — Gemini Error {}", resp.status),
                        content: format!(
                            "# Gemini Protocol Response: {}\n\n**Meta:** {}\n\n{}",
                            resp.status, resp.meta, resp.body
                        ),
                        direct_nodes: None,
                    }
                }
            }
            Err(err) => LoadedDocument {
                url: String::from(clean_url),
                title: String::from("Ocel — Gemini Error"),
                content: format!(
                    "# Gemini Protocol Error\n\nCould not fetch `{}`.\n\n**Reason:** {}\n\n### Suggestions:\n- Check port 1965 and network connection.\n- Ensure CellOS `service-net` TLS broker is active.",
                    clean_url, err
                ),
                direct_nodes: None,
            },
        };
    }


    // PDFs are binary and are owned/decoded only by the isolated PDF service.
    // Keep the original URL for history and links; fragments never reach VFS.
    if crate::pdf::is_pdf_path(path) {
        let pdf_path = path.split('#').next().unwrap_or(path);
        let result = crate::pdf::page_number(clean_url)
            .and_then(|page| crate::pdf::load_page(pdf_path, page));
        return match result {
            Ok(page) => LoadedDocument {
                url: String::from(clean_url),
                title: format!("Ocel — PDF ({})", pdf_path.rsplit('/').next().unwrap_or(pdf_path)),
                content: String::new(),
                direct_nodes: Some(page.into_nodes(clean_url)),
            },
            Err(error) => LoadedDocument {
                url: String::from(clean_url),
                title: String::from("Ocel — PDF Error"),
                content: String::new(),
                direct_nodes: Some(alloc::vec![
                    DocNode::Heading { level: 1, text: String::from("PDF Viewing Error") },
                    DocNode::Paragraph {
                        spans: alloc::vec![crate::doc::StyledSpan::plain(&format!("Could not view {}: {}", pdf_path, error))],
                    },
                ]),
            },
        };
    }

    // Binary image files must bypass the UTF-8 document reader.
    if crate::image::is_image_path(path) {
        match ostd::fs::File::open(path) {
            Ok(mut file) => {
                if let Some(bytes) = read_image_bytes(&mut file) {
                    if let Some(img) = crate::image::decode(&bytes) {
                        let filename = path.rsplit('/').next().unwrap_or(path);
                        return LoadedDocument {
                            url: String::from(clean_url),
                            title: format!("Ocel — Image ({})", filename),
                            content: String::new(),
                            direct_nodes: Some(alloc::vec![DocNode::Image {
                                width: img.width,
                                height: img.height,
                                pixels: alloc::rc::Rc::new(img.pixels),
                            }]),
                        };
                    }
                }
                return LoadedDocument {
                    url: String::from(clean_url),
                    title: String::from("Ocel — Image Decode Error"),
                    content: format!("# Error Decoding Image\n\nCould not decode `{}`. Supported formats: PNG, JPEG and BMP. Limits: 2 MiB encoded, 4096 pixels per dimension and 1,048,576 pixels total.", path),
                    direct_nodes: None,
                };
            }
            Err(_) => {
                return LoadedDocument {
                    url: String::from(clean_url),
                    title: String::from("Ocel — File Not Found"),
                    content: format!("# 404: Image Not Found\n\nCould not find `{}`.", path),
                    direct_nodes: None,
                };
            }
        }
    }

    // 3. Attempt to open text-based file from VFS
    match ostd::fs::File::open(path) {
        Ok(mut file) => match file.read_to_string() {
            Ok(content) => {
                let filename = path.rsplit('/').next().unwrap_or(path);
                LoadedDocument {
                    url: String::from(clean_url),
                    title: format!("Ocel — {}", filename),
                    content,
                    direct_nodes: None,
                }
            }
            Err(_) => LoadedDocument {
                url: String::from(clean_url),
                title: String::from("Ocel — Read Error"),
                content: format!(
                    "# Error Reading File\n\nCould not read data from `{}`.\n\nThe file might be non-UTF8 or corrupted.",
                    path
                ),
                direct_nodes: None,
            },
        },
        Err(_) => LoadedDocument {
            url: String::from(clean_url),
            title: String::from("Ocel — File Not Found"),
            content: format!(
                "# 404: Document Not Found\n\nCould not open `{}` from CellOS VFS.\n\n### Suggestions:\n- Verify the file path in `/data/` or `/srv/`.\n- Type `file:///welcome.md` to return to the welcome page.\n- Type `file:///help.md` for keyboard shortcuts.",
                path
            ),
            direct_nodes: None,
        },
    }
}

fn read_image_bytes(file: &mut ostd::fs::File) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let count = file.read(&mut chunk).ok()?;
        if count == 0 {
            return Some(bytes);
        }
        if bytes.len() + count > 2 * 1024 * 1024 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

fn get_welcome_content() -> String {
    String::from(
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
| Bộ nhớ | Viewer heap: 16 MiB; display and engine allocations are additional | Depends on guest and workload |
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
fn main() {
    println!("Hello from Ocel!");
}
```

*Built with passion for CellOS.*
"#,
    )
}

fn get_help_content() -> String {
    String::from(
        r#"# Ocel Keyboard & Navigation Help

### Keyboard Shortcuts
- **Up Arrow / Down Arrow:** Scroll viewport by 1 line (24 px).
- **PageUp / PageDown:** Scroll viewport by one screen height.
- **Home / End:** Jump to top or bottom of document.
- **Enter:** Navigate to URL typed in the address bar.
- **Backspace:** Edit or erase address bar characters.

---

### Supported URL Schemes
- `file:///path/to/file.md` — Open Markdown file from VFS.
- `file:///path/to/file.txt` — Open plain text or source code.
- `file:///path/to/file.html` — Open HTML document.
- `file:///path/to/file.pdf#page=2` — View a local PDF page (one-based; no fragment opens page 1).
- **PDF navigation:** Click Previous page / Next page above the raster. PageUp / PageDown still scroll; tab shortcuts are unchanged.
- **PDF availability:** Requires the registered native `ocel-pdf` service. One opaque page at a time, bounded to 1024 × 1024 pixels; the service accepts PDFs up to 2 MiB. No fallback renderer or PDF-source text view.
- `file:///welcome.md` — Built-in Welcome guide.
- `file:///help.md` — This help page.
"#,
    )
}

fn get_bookmarks_content() -> String {
    if let Ok(mut f) = ostd::fs::File::open("/data/bookmarks.md") {
        if let Ok(content) = f.read_to_string() {
            if !content.trim().is_empty() {
                return content;
            }
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
*Mẹo: Nhấn `Ctrl+D` để lưu trang hiện tại vào dấu trang này! Nhấn `Ctrl+B` để mở lại trang Bookmarks bất cứ lúc nào.*
"#,
    )
}

pub fn classify_smart_url(input: &str) -> String {
    let s = input.trim();
    if s.is_empty() {
        return String::from("file:///welcome.md");
    }

    if s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("file://")
        || s.starts_with("gemini://")
        || s.starts_with("ocel://")
    {
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
}
