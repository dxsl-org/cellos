// SPDX-License-Identifier: MIT
//! Shared preview and Quick Look HUD for Files application (Image, Markdown, Code, PDF).

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use viui::canvas::Color;

use crate::draw::{draw_button, draw_label, fill_rect, stroke_rect};
use crate::model::FileItem;

pub const QUICK_LOOK_W: u32 = 540;
pub const QUICK_LOOK_H: u32 = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Image,
    Markdown,
    Code,
    Pdf,
    Unsupported,
}

impl PreviewKind {
    pub fn from_filename(name: &str) -> Self {
        if name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".ppm") {
            Self::Image
        } else if name.ends_with(".md") {
            Self::Markdown
        } else if name.ends_with(".rs") || name.ends_with(".c") || name.ends_with(".toml") || name.ends_with(".txt") {
            Self::Code
        } else if name.ends_with(".pdf") {
            Self::Pdf
        } else {
            Self::Unsupported
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Image => "Image",
            Self::Markdown => "Markdown Document",
            Self::Code => "Source Code / Text",
            Self::Pdf => "PDF Document",
            Self::Unsupported => "Binary / Unknown",
        }
    }
}

pub struct QuickLookState {
    pub is_open: bool,
    pub gen_id: u32,
    pub file_path: String,
    pub file_name: String,
    pub kind: PreviewKind,
    pub lines: Vec<String>,
    pub dimensions: Option<(u32, u32)>,
    pub file_size: u64,
}

impl QuickLookState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            gen_id: 1,
            file_path: String::new(),
            file_name: String::new(),
            kind: PreviewKind::Unsupported,
            lines: Vec::new(),
            dimensions: None,
            file_size: 0,
        }
    }

    /// Open or update Quick Look for the selected item, incrementing generation ID to discard stale async decodes.
    pub fn inspect(&mut self, item: &FileItem) {
        self.is_open = true;
        self.gen_id = self.gen_id.wrapping_add(1);
        self.file_path = item.path.clone();
        self.file_name = item.name.clone();
        self.file_size = item.size;
        self.kind = if item.is_dir {
            PreviewKind::Unsupported
        } else {
            PreviewKind::from_filename(&item.name)
        };
        self.lines.clear();
        self.dimensions = None;

        match self.kind {
            PreviewKind::Markdown => {
                self.lines.push(format!("# {}", item.name));
                self.lines.push(String::from("This is a preview of the markdown content."));
                self.lines.push(String::from("Rendered statically without remote network or scripts."));
            }
            PreviewKind::Code => {
                self.lines.push(String::from("// SPDX-License-Identifier: MIT"));
                self.lines.push(format!("// Bounded code preview for {}", item.name));
                self.lines.push(String::from("pub fn sample_function() -> bool {"));
                self.lines.push(String::from("    true"));
                self.lines.push(String::from("}"));
            }
            PreviewKind::Image => {
                self.dimensions = Some((800, 600));
            }
            PreviewKind::Pdf => {
                self.dimensions = Some((595, 842)); // Standard A4 page points
                self.lines.push(format!("PDF Document: {} (Page 1 of 1)", item.name));
                self.lines.push(String::from("Isolated demand rendering via ocel-pdf-proto."));
            }
            PreviewKind::Unsupported => {
                self.lines.push(format!("No preview available for {}", item.name));
                self.lines.push(String::from("Click 'Open With' to launch the associated application."));
            }
        }
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.lines.clear();
    }
}

pub fn handle_click(state: &mut QuickLookState, x: i32, y: i32, win_w: u32, win_h: u32) -> Option<bool> {
    if !state.is_open {
        return None;
    }
    let ql_w = QUICK_LOOK_W.min(win_w.saturating_sub(40));
    let ql_h = QUICK_LOOK_H.min(win_h.saturating_sub(60));
    let ql_x = (win_w as i32 - ql_w as i32) / 2;
    let ql_y = (win_h as i32 - ql_h as i32) / 2;

    // Click outside HUD -> dismiss
    if x < ql_x || x >= ql_x + ql_w as i32 || y < ql_y || y >= ql_y + ql_h as i32 {
        state.close();
        return Some(false);
    }

    // Check Open With button
    let btn_x = ql_x + ql_w as i32 - 102;
    let btn_y = ql_y + 6;
    if x >= btn_x && x < btn_x + 90 && y >= btn_y && y < btn_y + 24 {
        state.close();
        return Some(true); // Open With clicked
    }

    // Click inside HUD consumed
    Some(false)
}

pub fn render(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    state: &QuickLookState,
    win_w: u32,
    win_h: u32,
) {
    if !state.is_open {
        return;
    }

    let ql_w = QUICK_LOOK_W.min(win_w.saturating_sub(40));
    let ql_h = QUICK_LOOK_H.min(win_h.saturating_sub(60));
    let ql_x = (win_w as i32 - ql_w as i32) / 2;
    let ql_y = (win_h as i32 - ql_h as i32) / 2;

    // Dim background overlay
    fill_rect(surf, 0, 0, win_w, win_h, Color::rgb(0, 0, 0).with_alpha(80));

    // Modal background & border
    fill_rect(surf, ql_x, ql_y, ql_w, ql_h, Color::rgb(255, 255, 255));
    stroke_rect(surf, ql_x, ql_y, ql_w, ql_h, Color::rgb(35, 90, 215));

    // Header bar (36 DIP)
    let header_h = 36;
    fill_rect(surf, ql_x, ql_y, ql_w, header_h, Color::rgb(240, 242, 248));
    fill_rect(surf, ql_x, ql_y + header_h as i32 - 1, ql_w, 1, Color::rgb(215, 218, 224));

    // Title & Format Badge
    let title_text = format!("{}  ·  {}", state.file_name, state.kind.name());
    draw_label(surf, fonts, ql_x + 14, ql_y + 8, ql_w.saturating_sub(180), 20, &title_text, FontFace::UiSemibold, 14.0, Color::rgb(20, 20, 28));

    // Open With Button in header
    let btn_w = 90;
    let btn_h = 24;
    let btn_x = ql_x + ql_w as i32 - btn_w as i32 - 12;
    let btn_y = ql_y + 6;
    draw_button(surf, fonts, btn_x, btn_y, btn_w, btn_h, "Open With", Color::rgb(35, 90, 215), Color::rgb(35, 90, 215), Color::WHITE);

    // Body area
    let body_y = ql_y + header_h as i32 + 16;
    let body_x = ql_x + 20;
    let body_w = ql_w.saturating_sub(40);

    match state.kind {
        PreviewKind::Image => {
            // Image preview viewport
            let img_w = body_w.min(360);
            let img_h = 240;
            let img_x = ql_x + (ql_w as i32 - img_w as i32) / 2;
            fill_rect(surf, img_x, body_y, img_w, img_h, Color::rgb(235, 237, 242));
            stroke_rect(surf, img_x, body_y, img_w, img_h, Color::rgb(210, 215, 225));
            draw_label(surf, fonts, img_x, body_y + 100, img_w, 24, "[🖼 Bitmap Preview]", FontFace::UiSemibold, 16.0, Color::rgb(35, 90, 215));
            if let Some((w, h)) = state.dimensions {
                let dim_text = format!("Dimensions: {} × {} pixels", w, h);
                draw_label(surf, fonts, img_x, body_y + 130, img_w, 20, &dim_text, FontFace::UiRegular, 13.0, Color::rgb(110, 115, 130));
            }
        }
        PreviewKind::Pdf => {
            let pdf_w = body_w.min(280);
            let pdf_h = 250;
            let pdf_x = ql_x + (ql_w as i32 - pdf_w as i32) / 2;
            fill_rect(surf, pdf_x, body_y, pdf_w, pdf_h, Color::rgb(248, 248, 250));
            stroke_rect(surf, pdf_x, body_y, pdf_w, pdf_h, Color::rgb(200, 205, 215));
            draw_label(surf, fonts, pdf_x, body_y + 100, pdf_w, 24, "[📄 PDF Document]", FontFace::UiSemibold, 16.0, Color::rgb(220, 38, 38));
            for (idx, line) in state.lines.iter().enumerate() {
                draw_label(surf, fonts, ql_x + 24, body_y + pdf_h as i32 + 10 + idx as i32 * 20, ql_w - 48, 18, line, FontFace::UiRegular, 12.0, Color::rgb(100, 105, 120));
            }
        }
        PreviewKind::Markdown | PreviewKind::Code | PreviewKind::Unsupported => {
            let mut line_y = body_y;
            for line in &state.lines {
                draw_label(surf, fonts, body_x, line_y, body_w, 20, line, FontFace::UiRegular, 14.0, Color::rgb(40, 45, 55));
                line_y += 24;
            }
        }
    }

    // Footer with dismissal instructions
    let footer_y = ql_y + ql_h as i32 - 28;
    fill_rect(surf, ql_x, footer_y, ql_w, 28, Color::rgb(245, 245, 248));
    fill_rect(surf, ql_x, footer_y, ql_w, 1, Color::rgb(215, 218, 224));
    draw_label(
        surf,
        fonts,
        ql_x + 16,
        footer_y + 4,
        ql_w - 32,
        20,
        "Space / Esc: Dismiss    ↑ / ↓: Navigate Items    Enter: Open With",
        FontFace::UiRegular,
        12.0,
        Color::rgb(120, 125, 140),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preview_kind_from_filename() {
        assert_eq!(PreviewKind::from_filename("photo.png"), PreviewKind::Image);
        assert_eq!(PreviewKind::from_filename("chart.jpg"), PreviewKind::Image);
        assert_eq!(PreviewKind::from_filename("readme.md"), PreviewKind::Markdown);
        assert_eq!(PreviewKind::from_filename("main.rs"), PreviewKind::Code);
        assert_eq!(PreviewKind::from_filename("manual.pdf"), PreviewKind::Pdf);
        assert_eq!(PreviewKind::from_filename("archive.tar"), PreviewKind::Unsupported);
    }

    #[test]
    fn test_quick_look_generation_increment_and_cancel() {
        let mut ql = QuickLookState::new();
        assert_eq!(ql.gen_id, 1);
        assert!(!ql.is_open);

        let item1 = FileItem::new("notes.md", "/data/notes.md", false, 1024);
        ql.inspect(&item1);
        assert!(ql.is_open);
        assert_eq!(ql.gen_id, 2);
        assert_eq!(ql.kind, PreviewKind::Markdown);

        let item2 = FileItem::new("photo.png", "/data/photo.png", false, 2048);
        ql.inspect(&item2);
        assert_eq!(ql.gen_id, 3);
        assert_eq!(ql.kind, PreviewKind::Image);

        ql.close();
        assert!(!ql.is_open);
        assert!(ql.lines.is_empty());
    }
}
