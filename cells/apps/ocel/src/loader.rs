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
    if path == "welcome" || path == "/welcome" || path == "/welcome.md" || path == "welcome.md" {
        return LoadedDocument {
            url: String::from("file:///welcome.md"),
            title: String::from("Ocel — Welcome Guide"),
            content: get_welcome_content(),
            direct_nodes: None,
        };
    }

    if path == "help" || path == "/help" || path == "/help.md" || path == "help.md" {
        return LoadedDocument {
            url: String::from("file:///help.md"),
            title: String::from("Ocel — Keyboard & Usage Help"),
            content: get_help_content(),
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

    // 2. Check if binary image (.bmp)
    if path.to_ascii_lowercase().ends_with(".bmp") {
        match ostd::fs::File::open(path) {
            Ok(mut file) => {
                let mut bytes = Vec::new();
                if file.read_to_end(&mut bytes).is_ok() {
                    if let Some(img) = crate::image::decode_bmp(&bytes) {
                        let filename = path.rsplit('/').next().unwrap_or(path);
                        return LoadedDocument {
                            url: String::from(clean_url),
                            title: format!("Ocel — Image ({})", filename),
                            content: String::new(),
                            direct_nodes: Some(alloc::vec![DocNode::Image {
                                width: img.width,
                                height: img.height,
                                pixels: img.pixels,
                            }]),
                        };
                    }
                }
                return LoadedDocument {
                    url: String::from(clean_url),
                    title: String::from("Ocel — Image Decode Error"),
                    content: format!("# Error Decoding BMP\n\nCould not decode `{}`.", path),
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

fn get_welcome_content() -> String {
    String::from(
        r#"# Welcome to Ocel

**Ocel** (derived from *Ocellus* — the simple eye) is the native document and web content viewer for **CellOS**.

---

### Key Capabilities
- **Lightweight & Fast:** Starts up in <100 ms with sub-10 MB memory footprint.
- **Universal Viewing:** Formats include Markdown, plain text, HTML/CSS subset, and PDF.
- **Dual Engine Architecture:** Ocel handles document viewing natively on Tier 2, while full web apps (like Gmail) run on Tier 3 Chrome.

### Tiếng Việt & Đa ngôn ngữ
Hỗ trợ đầy đủ bảng chữ cái tiếng Việt có dấu:
- **Nguyên âm:** á, à, ả, ã, ạ, ă, ắ, ằ, ẳ, ẵ, ặ, â, ấ, ầ, ẩ, ẫ, ậ...
- **Phụ âm đặc biệt:** đ, Đ
- **Đọc tài liệu:** Hiển thị trơn tru mọi tài liệu Markdown, HTML và văn bản tiếng Việt!


### So sánh Tính năng (Feature Matrix)

| Tiêu chí | Ocel (Tier 2) | Chrome (Tier 3) |
| -------- | ------------- | --------------- |
| Khởi động | < 100 ms | 5 - 10 giây |
| Bộ nhớ RAM | < 10 MB | > 256 MB |
| Định dạng | MD, HTML, PDF, BMP | Full Web Apps, SPA |
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
- `file:///welcome.md` — Built-in Welcome guide.
- `file:///help.md` — This help page.
"#,
    )
}
