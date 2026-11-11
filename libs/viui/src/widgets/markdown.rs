// SPDX-License-Identifier: MIT
//! MarkdownView widget — formatted native Markdown rendering for ViUI.

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::event::{Event, EventCx, EventStatus};
use crate::layout::{Constraints, LayoutNode, Point, Rect, Size};
use crate::widget::{PaintCx, ViWidget};
use crate::widgets::rich_text::{LaidOutWord, RichTextView, TextSpan};
use ostd::display::linear::PANEL_RADIUS;

/// Abstract parsed Markdown block node.
#[derive(Clone, Debug, PartialEq)]
pub enum MarkdownBlock {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph {
        spans: Vec<TextSpan>,
    },
    CodeBlock {
        lang: String,
        lines: Vec<String>,
    },
    ListItem {
        indent: u8,
        bullet: char,
        spans: Vec<TextSpan>,
    },
    Blockquote {
        spans: Vec<TextSpan>,
    },
    Rule,
}

/// Laid-out block geometry; colours resolve from the theme at paint time.
#[derive(Clone, Debug)]
pub(crate) enum LaidOutItem {
    Heading {
        rect: Rect,
        text: String,
        size_px: f32,
    },
    ParagraphWords(Vec<LaidOutWord>),
    CodeBlock {
        rect: Rect,
        _lang: String,
        lines: Vec<(Point, String)>,
        _line_h: f32,
        font_size: f32,
    },
    ListItem {
        bullet_pos: Point,
        bullet: char,
        words: Vec<LaidOutWord>,
    },
    Blockquote {
        bar_rect: Rect,
        words: Vec<LaidOutWord>,
    },
    Rule {
        p1: Point,
        p2: Point,
    },
}

/// MarkdownView parses and renders Markdown text using native ViCanvas drawing.
pub struct MarkdownView {
    pub raw_markdown: String,
    pub blocks: Vec<MarkdownBlock>,
    pub font_scale: f32,
    bounds: core::cell::Cell<Rect>,
    layout_cache: RefCell<Vec<LaidOutItem>>,
}

impl MarkdownView {
    pub fn new(markdown: impl Into<String>) -> Self {
        let raw = markdown.into();
        let blocks = Self::parse(&raw);
        Self {
            raw_markdown: raw,
            blocks,
            font_scale: 1.0,
            bounds: core::cell::Cell::new(Rect::ZERO),
            layout_cache: RefCell::new(Vec::new()),
        }
    }

    pub fn set_markdown(&mut self, markdown: impl Into<String>) {
        self.raw_markdown = markdown.into();
        self.blocks = Self::parse(&self.raw_markdown);
    }

    pub fn with_font_scale(mut self, scale: f32) -> Self {
        self.font_scale = scale;
        self
    }

    /// Parse inline Markdown formatting: `code`, **bold**, [link](url)
    pub fn parse_inlines(text: &str) -> Vec<TextSpan> {
        let mut spans = Vec::new();
        let mut chars = text.chars().peekable();
        let mut buf = String::new();

        while let Some(c) = chars.next() {
            match c {
                '`' => {
                    if !buf.is_empty() {
                        spans.push(TextSpan::plain(&buf));
                        buf.clear();
                    }
                    let mut code_buf = String::new();
                    for inner in chars.by_ref() {
                        if inner == '`' {
                            break;
                        }
                        code_buf.push(inner);
                    }
                    spans.push(TextSpan::code(&code_buf));
                }
                '*' if chars.peek() == Some(&'*') => {
                    chars.next(); // consume second '*'
                    if !buf.is_empty() {
                        spans.push(TextSpan::plain(&buf));
                        buf.clear();
                    }
                    let mut bold_buf = String::new();
                    while let Some(inner) = chars.next() {
                        if inner == '*' && chars.peek() == Some(&'*') {
                            chars.next(); // consume closing '*'
                            break;
                        }
                        bold_buf.push(inner);
                    }
                    spans.push(TextSpan::bold(&bold_buf));
                }
                '*' => {
                    if !buf.is_empty() {
                        spans.push(TextSpan::plain(&buf));
                        buf.clear();
                    }
                    let mut italic_buf = String::new();
                    for inner in chars.by_ref() {
                        if inner == '*' {
                            break;
                        }
                        italic_buf.push(inner);
                    }
                    spans.push(TextSpan::italic(&italic_buf));
                }
                '[' => {
                    if !buf.is_empty() {
                        spans.push(TextSpan::plain(&buf));
                        buf.clear();
                    }
                    let mut link_text = String::new();
                    for inner in chars.by_ref() {
                        if inner == ']' {
                            break;
                        }
                        link_text.push(inner);
                    }

                    let mut link_url = String::new();
                    if chars.peek() == Some(&'(') {
                        chars.next(); // consume '('
                        for inner in chars.by_ref() {
                            if inner == ')' {
                                break;
                            }
                            link_url.push(inner);
                        }
                    }

                    spans.push(TextSpan::link(&link_text, &link_url));
                }
                _ => {
                    buf.push(c);
                }
            }
        }

        if !buf.is_empty() {
            spans.push(TextSpan::plain(&buf));
        }

        spans
    }

    /// Parse markdown document into blocks.
    pub fn parse(input: &str) -> Vec<MarkdownBlock> {
        let mut blocks = Vec::new();
        let mut in_code_block = false;
        let mut code_lang = String::new();
        let mut code_lines = Vec::new();

        for line in input.lines() {
            let trimmed = line.trim();

            // Code fence toggle
            if trimmed.starts_with("```") {
                if in_code_block {
                    blocks.push(MarkdownBlock::CodeBlock {
                        lang: core::mem::take(&mut code_lang),
                        lines: core::mem::take(&mut code_lines),
                    });
                    in_code_block = false;
                } else {
                    in_code_block = true;
                    code_lang = String::from(trimmed.trim_start_matches("```").trim());
                }
                continue;
            }

            if in_code_block {
                code_lines.push(String::from(line));
                continue;
            }

            if trimmed.is_empty() {
                continue;
            }

            // Horizontal Rule
            if trimmed == "---" || trimmed == "***" || trimmed == "___" {
                blocks.push(MarkdownBlock::Rule);
                continue;
            }

            // Headings
            if let Some(rest) = trimmed.strip_prefix("# ") {
                blocks.push(MarkdownBlock::Heading {
                    level: 1,
                    text: rest.trim().to_string(),
                });
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("## ") {
                blocks.push(MarkdownBlock::Heading {
                    level: 2,
                    text: rest.trim().to_string(),
                });
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("### ") {
                blocks.push(MarkdownBlock::Heading {
                    level: 3,
                    text: rest.trim().to_string(),
                });
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("#### ") {
                blocks.push(MarkdownBlock::Heading {
                    level: 4,
                    text: rest.trim().to_string(),
                });
                continue;
            }

            // Blockquote
            if let Some(rest) = trimmed.strip_prefix("> ") {
                blocks.push(MarkdownBlock::Blockquote {
                    spans: Self::parse_inlines(rest),
                });
                continue;
            }

            // Lists
            let indent = ((line.len() - line.trim_start().len()) / 2) as u8;
            if let Some(rest) = trimmed.strip_prefix("- ") {
                blocks.push(MarkdownBlock::ListItem {
                    bullet: '*',
                    indent,
                    spans: Self::parse_inlines(rest),
                });
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("* ") {
                blocks.push(MarkdownBlock::ListItem {
                    bullet: '*',
                    indent,
                    spans: Self::parse_inlines(rest),
                });
                continue;
            }

            // Paragraph
            blocks.push(MarkdownBlock::Paragraph {
                spans: Self::parse_inlines(trimmed),
            });
        }

        if in_code_block && !code_lines.is_empty() {
            blocks.push(MarkdownBlock::CodeBlock {
                lang: code_lang,
                lines: code_lines,
            });
        }

        blocks
    }
}

impl ViWidget for MarkdownView {
    fn layout(
        &self,
        constraints: Constraints,
        font: &mut crate::font_context::FontContext,
    ) -> LayoutNode {
        let mut laid_out = self.layout_cache.borrow_mut();
        laid_out.clear();

        let base_font_size = font.size_px * self.font_scale;
        let mut cur_y = 0.0f32;
        let mut max_w = 0.0f32;
        let line_spacing = 4.0;

        for block in &self.blocks {
            match block {
                MarkdownBlock::Heading { level, text } => {
                    let h_size = match level {
                        1 => base_font_size * 1.5,
                        2 => base_font_size * 1.3,
                        3 => base_font_size * 1.15,
                        _ => base_font_size * 1.05,
                    };
                    let line_h = font.atlas.line_height(h_size);
                    let w = font.atlas.measure(text, h_size);

                    cur_y += 6.0; // Top margin for heading
                    let rect = Rect::new(
                        constraints.origin.x,
                        constraints.origin.y + cur_y,
                        w,
                        line_h,
                    );

                    laid_out.push(LaidOutItem::Heading {
                        rect,
                        text: text.clone(),
                        size_px: h_size,
                    });

                    cur_y += line_h + 4.0;
                    if w > max_w {
                        max_w = w;
                    }
                }
                MarkdownBlock::Paragraph { spans } => {
                    let mut words = Vec::new();
                    let origin = Point::new(constraints.origin.x, constraints.origin.y + cur_y);
                    let sz = RichTextView::layout_spans_into(
                        spans,
                        origin,
                        constraints.max.w,
                        line_spacing,
                        font,
                        &mut words,
                    );
                    laid_out.push(LaidOutItem::ParagraphWords(words));
                    cur_y += sz.h + 8.0;
                    if sz.w > max_w {
                        max_w = sz.w;
                    }
                }
                MarkdownBlock::CodeBlock { lang, lines } => {
                    let code_size = (base_font_size * 0.9).max(11.0);
                    let line_h = font.atlas.line_height(code_size);
                    let padding_x = 10.0;
                    let padding_y = 8.0;
                    let block_h = lines.len() as f32 * line_h + padding_y * 2.0;

                    let rect = Rect::new(
                        constraints.origin.x,
                        constraints.origin.y + cur_y,
                        constraints.max.w,
                        block_h,
                    );

                    let mut laid_lines = Vec::new();
                    for (i, line) in lines.iter().enumerate() {
                        let line_pos = Point::new(
                            constraints.origin.x + padding_x,
                            constraints.origin.y + cur_y + padding_y + i as f32 * line_h,
                        );
                        laid_lines.push((line_pos, line.clone()));
                    }

                    laid_out.push(LaidOutItem::CodeBlock {
                        rect,
                        _lang: lang.clone(),
                        lines: laid_lines,
                        _line_h: line_h,
                        font_size: code_size,
                    });

                    cur_y += block_h + 10.0;
                    if constraints.max.w > max_w {
                        max_w = constraints.max.w;
                    }
                }
                MarkdownBlock::ListItem {
                    indent,
                    bullet,
                    spans,
                } => {
                    let indent_px = *indent as f32 * 16.0 + 16.0;
                    let bullet_pos = Point::new(
                        constraints.origin.x + indent_px - 10.0,
                        constraints.origin.y + cur_y + 2.0,
                    );

                    let item_origin = Point::new(
                        constraints.origin.x + indent_px,
                        constraints.origin.y + cur_y,
                    );
                    let mut words = Vec::new();
                    let sz = RichTextView::layout_spans_into(
                        spans,
                        item_origin,
                        constraints.max.w - indent_px,
                        line_spacing,
                        font,
                        &mut words,
                    );

                    laid_out.push(LaidOutItem::ListItem {
                        bullet_pos,
                        bullet: *bullet,
                        words,
                    });

                    cur_y += sz.h + 4.0;
                    if (sz.w + indent_px) > max_w {
                        max_w = sz.w + indent_px;
                    }
                }
                MarkdownBlock::Blockquote { spans } => {
                    let indent_px = 14.0;
                    let mut words = Vec::new();
                    let quote_origin = Point::new(
                        constraints.origin.x + indent_px,
                        constraints.origin.y + cur_y,
                    );
                    let sz = RichTextView::layout_spans_into(
                        spans,
                        quote_origin,
                        constraints.max.w - indent_px,
                        line_spacing,
                        font,
                        &mut words,
                    );

                    let bar_rect = Rect::new(
                        constraints.origin.x,
                        constraints.origin.y + cur_y,
                        3.0,
                        sz.h,
                    );

                    laid_out.push(LaidOutItem::Blockquote { bar_rect, words });
                    cur_y += sz.h + 8.0;
                    if (sz.w + indent_px) > max_w {
                        max_w = sz.w + indent_px;
                    }
                }
                MarkdownBlock::Rule => {
                    let line_y = constraints.origin.y + cur_y + 6.0;
                    let p1 = Point::new(constraints.origin.x, line_y);
                    let p2 = Point::new(constraints.origin.x + constraints.max.w, line_y);

                    laid_out.push(LaidOutItem::Rule { p1, p2 });

                    cur_y += 12.0;
                }
            }
        }

        let desired = Size {
            w: max_w.min(constraints.max.w),
            h: cur_y,
        };

        let size = constraints.constrain(desired);
        let bounds = Rect::from_origin_size(constraints.origin, size);
        self.bounds.set(bounds);
        LayoutNode::leaf(bounds)
    }

    fn paint(&self, cx: &mut PaintCx) {
        let theme = cx.theme;
        let items = self.layout_cache.borrow();
        for item in items.iter() {
            match item {
                LaidOutItem::Heading {
                    rect,
                    text,
                    size_px,
                } => {
                    let screen_pos = rect.origin().offset(cx.origin.x, cx.origin.y);
                    cx.draw_text(screen_pos, text, theme.text_primary(), *size_px);
                }
                LaidOutItem::ParagraphWords(words) => {
                    RichTextView::paint_words(words, theme.text_primary(), cx);
                }
                LaidOutItem::CodeBlock {
                    rect,
                    lines,
                    font_size,
                    ..
                } => {
                    let screen_rect = cx.screen_rect(*rect);
                    cx.canvas
                        .fill_round_rect(screen_rect, PANEL_RADIUS, theme.surface());
                    cx.canvas
                        .stroke_round_rect(screen_rect, PANEL_RADIUS, theme.border());
                    for (pos, line_str) in lines {
                        let screen_pos = pos.offset(cx.origin.x, cx.origin.y);
                        cx.draw_text(screen_pos, line_str, theme.text_primary(), *font_size);
                    }
                }
                LaidOutItem::ListItem {
                    bullet_pos,
                    bullet,
                    words,
                } => {
                    let screen_bullet_pos = bullet_pos.offset(cx.origin.x, cx.origin.y);
                    let mut b_str = String::new();
                    b_str.push(*bullet);
                    let size = cx.font.size_px;
                    cx.draw_text(screen_bullet_pos, &b_str, theme.text_secondary(), size);
                    RichTextView::paint_words(words, theme.text_primary(), cx);
                }
                LaidOutItem::Blockquote { bar_rect, words } => {
                    let screen_bar = cx.screen_rect(*bar_rect);
                    cx.canvas.fill_rect(screen_bar, theme.accent());
                    RichTextView::paint_words(words, theme.text_secondary(), cx);
                }
                LaidOutItem::Rule { p1, p2 } => {
                    let sp1 = p1.offset(cx.origin.x, cx.origin.y);
                    let sp2 = p2.offset(cx.origin.x, cx.origin.y);
                    cx.canvas.draw_line(sp1, sp2, theme.divider());
                }
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
    fn test_markdown_inline_parsing() {
        let text = "Text with **bold** and *italic* and `code` and [link](https://cellos.org)";
        let spans = MarkdownView::parse_inlines(text);

        assert!(spans.iter().any(|s| s.text == "bold" && s.bold));
        assert!(spans.iter().any(|s| s.text == "italic" && s.italic));
        assert!(spans.iter().any(|s| s.text == "code" && s.code));
        assert!(spans
            .iter()
            .any(|s| s.text == "link" && s.link.as_deref() == Some("https://cellos.org")));
    }

    #[test]
    fn test_markdown_block_parsing() {
        let md = "# Title\n\n## Subtitle\n\nParagraph text\n\n```rust\nfn main() {}\n```\n\n- Item 1\n- Item 2\n\n> Quoted note\n\n---";
        let blocks = MarkdownView::parse(md);

        assert!(blocks
            .iter()
            .any(|b| matches!(b, MarkdownBlock::Heading { level: 1, text } if text == "Title")));
        assert!(blocks
            .iter()
            .any(|b| matches!(b, MarkdownBlock::Heading { level: 2, text } if text == "Subtitle")));
        assert!(blocks.iter().any(|b| matches!(b, MarkdownBlock::CodeBlock { lang, lines } if lang == "rust" && lines[0] == "fn main() {}")));
        assert!(blocks
            .iter()
            .any(|b| matches!(b, MarkdownBlock::ListItem { bullet: '*', .. })));
        assert!(blocks
            .iter()
            .any(|b| matches!(b, MarkdownBlock::Blockquote { .. })));
        assert!(blocks.iter().any(|b| matches!(b, MarkdownBlock::Rule)));
    }

    #[test]
    fn test_markdown_view_layout() {
        let mut font = FontContext::default();
        let md = "# Header 1\n\nHere is a paragraph with **bold** text.\n\n```python\nprint('hello')\n```\n\n- Bullet item";
        let view = MarkdownView::new(md);

        let constraints = Constraints::new(Point::ZERO, Size::new(400.0, 1000.0));
        let node = view.layout(constraints, &mut font);

        assert!(node.bounds.w > 0.0);
        assert!(node.bounds.h > 50.0);
        assert!(!view.layout_cache.borrow().is_empty());
    }

    #[test]
    fn blocks_resolve_colours_from_the_theme() {
        use crate::node_widgets::style::test_support::{fills, lines, record, strokes, text_color};
        use crate::theme::{ViTheme, DARK_THEME, LIGHT_THEME};
        use crate::widget::PaintCx;

        let md = "# Title\n\nBody\n\n```\nlet x\n```\n\n- Item\n\n> Quote\n\n---";
        for theme in [&LIGHT_THEME as &dyn ViTheme, &DARK_THEME] {
            let view = MarkdownView::new(md);
            let mut font = FontContext::default();
            view.layout(
                Constraints::new(Point::ZERO, Size::new(400.0, 1000.0)),
                &mut font,
            );
            let buf =
                record(|canvas| view.paint(&mut PaintCx::with_theme(canvas, theme, &mut font)));
            for text in ["Title", "Body", "let x", "Item"] {
                assert_eq!(text_color(&buf, text), Some(theme.text_primary()), "{text}");
            }
            assert_eq!(text_color(&buf, "*"), Some(theme.text_secondary()));
            assert_eq!(text_color(&buf, "Quote"), Some(theme.text_secondary()));
            let painted = fills(&buf);
            assert_eq!(painted[0].1, theme.surface());
            assert_eq!(painted[1].1, theme.accent());
            assert_eq!(strokes(&buf), [theme.border()]);
            assert_eq!(lines(&buf), [theme.divider()]);
        }
    }
}
