// SPDX-License-Identifier: MIT
//! Quick Look HUD for Files. Text documents show the first lines of the file as read from
//! the VFS (bounded read); every other kind shows its real size and no fabricated content.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use viui::canvas::Color;
use viui::theme::ViTheme;

use crate::draw::{draw_label, fill_rect, stroke_rect};
use crate::fs::FsResult;
use crate::model::FileItem;

pub const QUICK_LOOK_W: u32 = 540;
pub const QUICK_LOOK_H: u32 = 400;
/// Text lines and columns the HUD body can show.
const PREVIEW_LINES: usize = 12;
const PREVIEW_COLS: usize = 72;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Folder,
    Image,
    Markdown,
    Code,
    Pdf,
    Unsupported,
}

impl PreviewKind {
    pub fn of(item: &FileItem) -> Self {
        if item.is_dir {
            Self::Folder
        } else {
            match item.mime {
                wire::mime::MimeType::ImagePng
                | wire::mime::MimeType::ImageJpeg
                | wire::mime::MimeType::ImagePpm => Self::Image,
                wire::mime::MimeType::TextMarkdown => Self::Markdown,
                wire::mime::MimeType::TextPlain | wire::mime::MimeType::TextSource => Self::Code,
                wire::mime::MimeType::ApplicationPdf => Self::Pdf,
                _ => Self::Unsupported,
            }
        }
    }

    /// Whether Quick Look reads and shows the file's text.
    pub const fn is_text(self) -> bool {
        matches!(self, Self::Markdown | Self::Code)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Folder => "Folder",
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
    pub file_name: String,
    pub file_path: String,
    pub kind: PreviewKind,
    pub mime: wire::mime::MimeType,
    pub default_app: Option<&'static str>,
    pub candidates: &'static [wire::mime::AppCandidate],
    pub lines: Vec<String>,
}

impl QuickLookState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            file_name: String::new(),
            file_path: String::new(),
            kind: PreviewKind::Unsupported,
            mime: wire::mime::MimeType::Unknown,
            default_app: None,
            candidates: &[],
            lines: Vec::new(),
        }
    }

    /// Open (or retarget) Quick Look on `item`. `text` is the VFS read of a text kind and
    /// `None` for kinds that are not read.
    pub fn show(&mut self, item: &FileItem, text: Option<FsResult<Vec<u8>>>) {
        self.is_open = true;
        self.file_name = item.name.clone();
        self.file_path = item.path.clone();
        self.kind = PreviewKind::of(item);
        self.mime = item.mime;
        self.default_app =
            wire::mime::AssociationRegistry::default_app(item.mime).map(|a| a.app_name);
        self.candidates = wire::mime::AssociationRegistry::candidates_for_mime(item.mime);
        self.lines.clear();
        match text {
            Some(Ok(bytes)) if bytes.is_empty() => self.lines.push(String::from("(empty file)")),
            Some(Ok(bytes)) => {
                let text = String::from_utf8_lossy(&bytes);
                self.lines.extend(
                    text.lines()
                        .take(PREVIEW_LINES)
                        .map(|line| line.chars().take(PREVIEW_COLS).collect::<String>()),
                );
            }
            Some(Err(e)) => self
                .lines
                .push(format!("Cannot read {}: {}", item.name, e.describe())),
            None if item.is_dir => self
                .lines
                .push(String::from("Folder. Press Enter to open it.")),
            None => {
                self.lines
                    .push(format!("No inline preview for {}.", self.kind.name()));
                self.lines.push(format!("Size: {}", item.formatted_size()));
            }
        }
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.lines.clear();
    }
}

fn hud_rect(win_w: u32, win_h: u32) -> (i32, i32, u32, u32) {
    let ql_w = QUICK_LOOK_W.min(win_w.saturating_sub(40));
    let ql_h = QUICK_LOOK_H.min(win_h.saturating_sub(60));
    (
        (win_w as i32 - ql_w as i32) / 2,
        (win_h as i32 - ql_h as i32) / 2,
        ql_w,
        ql_h,
    )
}

/// A click while Quick Look is open: outside the HUD dismisses it. Returns whether the
/// HUD consumed the click.
pub fn handle_click(state: &mut QuickLookState, x: i32, y: i32, win_w: u32, win_h: u32) -> bool {
    if !state.is_open {
        return false;
    }
    let (ql_x, ql_y, ql_w, ql_h) = hud_rect(win_w, win_h);
    if x < ql_x || x >= ql_x + ql_w as i32 || y < ql_y || y >= ql_y + ql_h as i32 {
        state.close();
    }
    true
}

pub fn render(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    theme: &dyn ViTheme,
    state: &QuickLookState,
    win_w: u32,
    win_h: u32,
) {
    if !state.is_open {
        return;
    }
    let (ql_x, ql_y, ql_w, ql_h) = hud_rect(win_w, win_h);

    // Dim scrim over the window: black at low alpha darkens every palette alike.
    fill_rect(surf, 0, 0, win_w, win_h, Color::BLACK.with_alpha(80));

    // Modal background & border
    fill_rect(surf, ql_x, ql_y, ql_w, ql_h, theme.surface());
    stroke_rect(surf, ql_x, ql_y, ql_w, ql_h, theme.accent());

    // Header bar (36 DIP)
    let header_h = 36;
    fill_rect(surf, ql_x, ql_y, ql_w, header_h, theme.bg());
    fill_rect(
        surf,
        ql_x,
        ql_y + header_h as i32 - 1,
        ql_w,
        1,
        theme.border(),
    );

    let title_text = format!("{}  ·  {}", state.file_name, state.kind.name());
    draw_label(
        surf,
        fonts,
        ql_x + 14,
        ql_y + 8,
        ql_w.saturating_sub(28),
        20,
        &title_text,
        FontFace::UiSemibold,
        14.0,
        theme.text_primary(),
    );

    let body_x = ql_x + 20;
    let body_w = ql_w.saturating_sub(40);
    let (face, size) = if state.kind.is_text() {
        (FontFace::MonoRegular, 13.0)
    } else {
        (FontFace::UiRegular, 14.0)
    };
    let mut line_y = ql_y + header_h as i32 + 16;
    for line in &state.lines {
        draw_label(
            surf,
            fonts,
            body_x,
            line_y,
            body_w,
            20,
            line,
            face,
            size,
            theme.text_primary(),
        );
        line_y += 24;
    }

    // Footer with dismissal instructions
    let footer_y = ql_y + ql_h as i32 - 28;
    fill_rect(surf, ql_x, footer_y, ql_w, 28, theme.bg());
    fill_rect(surf, ql_x, footer_y, ql_w, 1, theme.border());
    let footer_text = if let Some(app) = state.default_app {
        if state.mime.is_executable() {
            format!("Enter: Run in {}    Space/Esc: Dismiss", app)
        } else if state.candidates.len() > 1 {
            format!(
                "Enter: Open ({})  ·  1..{}: Open With  ·  Esc: Dismiss",
                app,
                state.candidates.len()
            )
        } else {
            format!("Enter: Open with {}    Space/Esc: Dismiss", app)
        }
    } else {
        String::from("Space / Esc: Dismiss    ↑ / ↓: Navigate Items")
    };
    draw_label(
        surf,
        fonts,
        ql_x + 16,
        footer_y + 4,
        ql_w - 32,
        20,
        &footer_text,
        FontFace::UiRegular,
        12.0,
        theme.text_secondary(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FsError;
    use ostd::ViError;

    fn file(name: &str) -> FileItem {
        FileItem::new(name, alloc::format!("/data/{name}"), false, Some(5))
    }

    #[test]
    fn preview_kind_from_entry() {
        assert_eq!(PreviewKind::of(&file("photo.png")), PreviewKind::Image);
        assert_eq!(PreviewKind::of(&file("readme.md")), PreviewKind::Markdown);
        assert_eq!(PreviewKind::of(&file("a.txt")), PreviewKind::Code);
        assert_eq!(PreviewKind::of(&file("manual.pdf")), PreviewKind::Pdf);
        assert_eq!(
            PreviewKind::of(&file("archive.tar")),
            PreviewKind::Unsupported
        );
        assert_eq!(
            PreviewKind::of(&FileItem::new("d", "/d", true, None)),
            PreviewKind::Folder
        );
    }

    #[test]
    fn shows_read_text_bounded_and_reports_failures() {
        let mut ql = QuickLookState::new();
        let long = "x".repeat(PREVIEW_COLS + 10);
        let mut text = alloc::format!("first\n{long}\n");
        for i in 0..PREVIEW_LINES {
            text.push_str(&alloc::format!("l{i}\n"));
        }
        ql.show(&file("a.txt"), Some(Ok(text.into_bytes())));
        assert!(ql.is_open);
        assert_eq!(ql.lines.len(), PREVIEW_LINES);
        assert_eq!(ql.lines[0], "first");
        assert_eq!(ql.lines[1].chars().count(), PREVIEW_COLS);

        ql.show(
            &file("a.txt"),
            Some(Err(FsError::Vfs(ViError::PermissionDenied))),
        );
        assert_eq!(
            ql.lines,
            ["Cannot read a.txt: permission denied (VFS ERR_DENIED)"]
        );

        ql.show(&file("photo.png"), None);
        assert_eq!(ql.lines, ["No inline preview for Image.", "Size: 5 B"]);

        assert!(handle_click(&mut ql, 10, 10, 800, 600));
        assert!(!ql.is_open);
        assert!(!handle_click(&mut ql, 10, 10, 800, 600));
    }
}
