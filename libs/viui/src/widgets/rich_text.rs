// SPDX-License-Identifier: MIT
//! RichTextView widget — inline styled multi-span text with word wrapping.

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::canvas::Color;
use crate::event::{Event, EventCx, EventStatus};
use crate::layout::{Constraints, LayoutNode, Point, Rect, Size};
use crate::widget::{PaintCx, ViWidget};

/// An individual formatted span of text within a rich text view.
#[derive(Clone, Debug, PartialEq)]
pub struct TextSpan {
    pub text: String,
    pub color: Option<Color>,
    pub bg_color: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub size_px: Option<f32>,
    pub link: Option<String>,
}

impl TextSpan {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: None,
            bg_color: None,
            bold: false,
            italic: false,
            code: false,
            size_px: None,
            link: None,
        }
    }

    pub fn bold(text: impl Into<String>) -> Self {
        let mut s = Self::plain(text);
        s.bold = true;
        s
    }

    pub fn italic(text: impl Into<String>) -> Self {
        let mut s = Self::plain(text);
        s.italic = true;
        s
    }

    pub fn code(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: Some(Color::CYAN),
            bg_color: Some(Color::bgra(45, 38, 30, 255)), // Subtle dark highlight
            bold: false,
            italic: false,
            code: true,
            size_px: None,
            link: None,
        }
    }

    pub fn link(text: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: Some(Color::bgra(240, 160, 60, 255)), // Vibrant accent link blue
            bg_color: None,
            bold: false,
            italic: false,
            code: false,
            size_px: None,
            link: Some(url.into()),
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_bg_color(mut self, bg: Color) -> Self {
        self.bg_color = Some(bg);
        self
    }

    pub fn with_size(mut self, size_px: f32) -> Self {
        self.size_px = Some(size_px);
        self
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LaidOutWord {
    pub rect: Rect,
    pub text: String,
    pub color: Color,
    pub bg_color: Option<Color>,
    pub size_px: f32,
    pub _is_code: bool,
    pub is_link: bool,
}

/// RichTextView displays styled text spans with automatic word wrapping.
pub struct RichTextView {
    pub spans: Vec<TextSpan>,
    pub line_spacing: f32,
    pub default_color: Option<Color>,
    bounds: core::cell::Cell<Rect>,
    layout_cache: RefCell<Vec<LaidOutWord>>,
}

impl RichTextView {
    pub fn new(spans: Vec<TextSpan>) -> Self {
        Self {
            spans,
            line_spacing: 4.0,
            default_color: None,
            bounds: core::cell::Cell::new(Rect::ZERO),
            layout_cache: RefCell::new(Vec::new()),
        }
    }

    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn push(&mut self, span: TextSpan) {
        self.spans.push(span);
    }

    pub fn with_line_spacing(mut self, spacing: f32) -> Self {
        self.line_spacing = spacing;
        self
    }

    pub fn with_default_color(mut self, color: Color) -> Self {
        self.default_color = Some(color);
        self
    }

    /// Internal layout calculation for wrapping spans within a given max width.
    pub(crate) fn layout_spans_into(
        spans: &[TextSpan],
        origin: Point,
        max_w: f32,
        line_spacing: f32,
        default_color: Color,
        font: &mut crate::font_context::FontContext,
        out_words: &mut Vec<LaidOutWord>,
    ) -> Size {
        out_words.clear();

        let base_font_size = font.size_px;
        let mut cur_x = 0.0f32;
        let mut cur_y = 0.0f32;
        let mut max_row_w = 0.0f32;
        let mut cur_row_h = font.atlas.line_height(base_font_size);

        for span in spans {
            let size_px = span.size_px.unwrap_or(base_font_size);
            let line_h = font.atlas.line_height(size_px);
            if line_h > cur_row_h {
                cur_row_h = line_h;
            }

            let text_color = span.color.unwrap_or(default_color);
            let is_link = span.link.is_some();

            // Handle newline or split words
            for paragraph_part in span.text.split('\n') {
                let words: Vec<&str> = paragraph_part.split(' ').collect();
                let word_count = words.len();

                for (idx, &word) in words.iter().enumerate() {
                    if word.is_empty() {
                        if idx + 1 < word_count {
                            // Empty space between multiple spaces
                            let space_w = font.atlas.measure(" ", size_px);
                            cur_x += space_w;
                        }
                        continue;
                    }

                    let word_w = font.atlas.measure(word, size_px);
                    let space_w = if idx + 1 < word_count {
                        font.atlas.measure(" ", size_px)
                    } else {
                        0.0
                    };

                    // Line wrap check
                    if cur_x > 0.0 && (cur_x + word_w) > max_w {
                        cur_x = 0.0;
                        cur_y += cur_row_h + line_spacing;
                        cur_row_h = line_h;
                    }

                    let word_rect = Rect::new(
                        origin.x + cur_x,
                        origin.y + cur_y,
                        word_w,
                        line_h,
                    );

                    out_words.push(LaidOutWord {
                        rect: word_rect,
                        text: word.to_string(),
                        color: text_color,
                        bg_color: span.bg_color,
                        size_px,
                        _is_code: span.code,
                        is_link,
                    });

                    cur_x += word_w + space_w;
                    if cur_x > max_row_w {
                        max_row_w = cur_x;
                    }
                }
            }
        }

        let total_h = if out_words.is_empty() {
            0.0
        } else {
            cur_y + cur_row_h
        };

        Size {
            w: max_row_w.min(max_w),
            h: total_h,
        }
    }
}

impl ViWidget for RichTextView {
    fn layout(&self, constraints: Constraints, font: &mut crate::font_context::FontContext) -> LayoutNode {
        let mut words = self.layout_cache.borrow_mut();
        let fallback_color = self.default_color.unwrap_or(Color::WHITE);
        let desired = Self::layout_spans_into(
            &self.spans,
            constraints.origin,
            constraints.max.w,
            self.line_spacing,
            fallback_color,
            font,
            &mut words,
        );

        let size = constraints.constrain(desired);
        let bounds = Rect::from_origin_size(constraints.origin, size);
        self.bounds.set(bounds);
        LayoutNode::leaf(bounds)
    }

    fn paint(&self, cx: &mut PaintCx) {
        let words = self.layout_cache.borrow();
        for word in words.iter() {
            let screen_rect = cx.screen_rect(word.rect);

            // Draw background badge (e.g. for inline code)
            if let Some(bg) = word.bg_color {
                let pad = 2.0;
                let badge_rect = Rect::new(
                    screen_rect.x - pad,
                    screen_rect.y,
                    screen_rect.w + pad * 2.0,
                    screen_rect.h,
                );
                cx.canvas.fill_rect(badge_rect, bg);
            }

            // Draw text
            let pos = screen_rect.origin();
            cx.draw_text(pos, &word.text, word.color, word.size_px);

            // Draw link underline
            if word.is_link {
                let underline_y = screen_rect.y + screen_rect.h - 1.0;
                let p1 = Point::new(screen_rect.x, underline_y);
                let p2 = Point::new(screen_rect.x + screen_rect.w, underline_y);
                cx.canvas.draw_line(p1, p2, word.color);
            }
        }
    }

    fn event(&mut self, _cx: &mut EventCx, _e: &Event) -> EventStatus {
        EventStatus::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font_context::FontContext;

    #[test]
    fn test_text_span_constructors() {
        let s1 = TextSpan::plain("Hello");
        assert_eq!(s1.text, "Hello");
        assert!(!s1.bold);
        assert!(!s1.code);

        let s2 = TextSpan::bold("World");
        assert!(s2.bold);

        let s3 = TextSpan::code("cargo check");
        assert!(s3.code);
        assert_eq!(s3.color, Some(Color::CYAN));

        let s4 = TextSpan::link("CellOS", "https://cellos.org");
        assert_eq!(s4.link.as_deref(), Some("https://cellos.org"));
    }

    #[test]
    fn test_rich_text_wrapping() {
        let mut font = FontContext::default();
        let spans = alloc::vec![
            TextSpan::plain("The quick brown fox jumps over the lazy dog."),
            TextSpan::code("fn main()"),
        ];
        let view = RichTextView::new(spans);

        // Narrow constraint should wrap into multiple rows
        let constraints = Constraints::new(Point::ZERO, Size::new(100.0, 500.0));
        let node = view.layout(constraints, &mut font);

        assert!(node.bounds.w <= 100.0);
        assert!(node.bounds.h > font.line_height());
        assert!(view.layout_cache.borrow().len() >= 9);
    }
}
