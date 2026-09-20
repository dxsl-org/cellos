// SPDX-License-Identifier: MIT
//! Document representation, styling, and layout engine for Ocel.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::draw::{self, fill_rect, stroke_rect, theme, Color};
use ostd::display::ViSurface;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: Option<String>,
    pub color: Option<Color>,
}

impl StyledSpan {
    pub fn plain(text: &str) -> Self {
        Self {
            text: String::from(text),
            bold: false,
            italic: false,
            code: false,
            link: None,
            color: None,
        }
    }

    pub fn code(text: &str) -> Self {
        Self {
            text: String::from(text),
            bold: false,
            italic: false,
            code: true,
            link: None,
            color: Some(theme::ACCENT_CYAN),
        }
    }
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum DocNode {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph {
        spans: Vec<StyledSpan>,
    },
    CodeBlock {
        lang: String,
        lines: Vec<String>,
    },
    ListItem {
        bullet: char,
        indent: u8,
        spans: Vec<StyledSpan>,
    },
    Blockquote {
        lines: Vec<String>,
    },
    Rule,
    RawLines {
        lines: Vec<String>,
    },
    Image {
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Button {
        text: String,
        node_id: Option<dom_arena::NodeId>,
    },
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct LayoutLine {
    pub spans: Vec<StyledSpan>,
    pub is_code: bool,
    pub is_heading: bool,
    pub scale: u32,
    pub custom_color: Option<Color>,
}

#[derive(Clone, Debug)]
pub struct TableCellLayout {
    pub text: String,
    pub x: i32,
    pub width: u32,
}

#[derive(Clone, Debug)]
pub struct TableRowLayout {
    pub y_rel: i32,
    pub height: u32,
    pub cells: Vec<TableCellLayout>,
    pub is_header: bool,
}

#[derive(Clone, Debug)]
pub struct LayoutBox {
    pub y_offset: i32,
    pub height: i32,
    pub lines: Vec<LayoutLine>,
    pub bg_color: Option<Color>,
    pub border_color: Option<Color>,
    pub is_rule: bool,
    pub left_margin: i32,
    pub image: Option<(u32, u32, Vec<u8>)>,
    pub table_rows: Option<Vec<TableRowLayout>>,
    pub table_width: u32,
    pub node_id: Option<dom_arena::NodeId>,
}

pub struct Document {
    pub nodes: Vec<DocNode>,
    pub layout_boxes: Vec<LayoutBox>,
    pub total_height: i32,
    pub arena: Option<dom_arena::DocumentArena>,
}

impl Document {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            layout_boxes: Vec::new(),
            total_height: 0,
            arena: None,
        }
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.layout_boxes.clear();
        self.total_height = 0;
    }

    /// Compute pixel layout coordinates for all document nodes given viewport width.
    pub fn compute_layout(&mut self, content_width: u32) {
        self.layout_boxes.clear();
        let mut curr_y = 16i32;
        let line_width_chars = (content_width.saturating_sub(40) / 8) as usize;
        let wrap_chars = line_width_chars.max(20);

        for node in &self.nodes {
            match node {
                DocNode::Heading { level, text } => {
                    let scale = match level {
                        1 => 2,
                        2 => 2,
                        _ => 1,
                    };
                    let line_height = (8 * scale + 6) as i32;
                    let color = if *level == 1 {
                        theme::ACCENT_BLUE
                    } else {
                        theme::TEXT_PRIMARY
                    };

                    let box_item = LayoutBox {
                        y_offset: curr_y,
                        height: line_height + 8,
                        lines: alloc::vec![LayoutLine {
                            spans: alloc::vec![StyledSpan::plain(text)],
                            is_code: false,
                            is_heading: true,
                            scale,
                            custom_color: Some(color),
                        }],
                        bg_color: None,
                        border_color: None,
                        is_rule: false,
                        left_margin: 0,
                        image: None,
                        table_rows: None,
                        table_width: 0,
                        node_id: None,
                    };

                    curr_y += box_item.height;
                    self.layout_boxes.push(box_item);
                }

                DocNode::Paragraph { spans } => {
                    // Wrap spans into lines
                    let mut lines = Vec::new();
                    let mut current_line_spans = Vec::new();
                    let mut current_len = 0;

                    for span in spans {
                        let words = span.text.split(' ');
                        for (i, word) in words.enumerate() {
                            let prefix = if i > 0 { " " } else { "" };
                            let word_len = word.len() + prefix.len();

                            if current_len + word_len > wrap_chars && current_len > 0 {
                                lines.push(LayoutLine {
                                    spans: core::mem::take(&mut current_line_spans),
                                    is_code: false,
                                    is_heading: false,
                                    scale: 1,
                                    custom_color: None,
                                });
                                current_len = 0;
                            }

                            let mut piece = span.clone();
                            piece.text = format!("{}{}", prefix, word);
                            current_len += word_len;
                            current_line_spans.push(piece);
                        }
                    }

                    if !current_line_spans.is_empty() {
                        lines.push(LayoutLine {
                            spans: current_line_spans,
                            is_code: false,
                            is_heading: false,
                            scale: 1,
                            custom_color: None,
                        });
                    }

                    let line_count = lines.len().max(1);
                    let height = (line_count as i32) * 16 + 6;

                    let box_item = LayoutBox {
                        y_offset: curr_y,
                        height,
                        lines,
                        bg_color: None,
                        border_color: None,
                        is_rule: false,
                        left_margin: 0,
                        image: None,
                        table_rows: None,
                        table_width: 0,
                        node_id: None,
                    };

                    curr_y += height;
                    self.layout_boxes.push(box_item);
                }

                DocNode::CodeBlock { lines, .. } => {
                    let mut layout_lines = Vec::new();
                    for l in lines {
                        layout_lines.push(LayoutLine {
                            spans: alloc::vec![StyledSpan::code(l)],
                            is_code: true,
                            is_heading: false,
                            scale: 1,
                            custom_color: Some(theme::ACCENT_CYAN),
                        });
                    }

                    let height = (lines.len() as i32) * 14 + 16;
                    let box_item = LayoutBox {
                        y_offset: curr_y,
                        height,
                        lines: layout_lines,
                        bg_color: Some(theme::BG_INPUT),
                        border_color: Some(theme::BORDER),
                        is_rule: false,
                        left_margin: 8,
                        image: None,
                        table_rows: None,
                        table_width: 0,
                        node_id: None,
                    };

                    curr_y += height + 8;
                    self.layout_boxes.push(box_item);
                }

                DocNode::ListItem {
                    bullet,
                    indent,
                    spans,
                } => {
                    let indent_px = (*indent as i32) * 16 + 12;
                    let mut spans_with_bullet = Vec::new();
                    spans_with_bullet.push(StyledSpan {
                        text: format!("{} ", bullet),
                        bold: true,
                        italic: false,
                        code: false,
                        link: None,
                        color: Some(theme::ACCENT_CYAN),
                    });
                    spans_with_bullet.extend_from_slice(spans);

                    let box_item = LayoutBox {
                        y_offset: curr_y,
                        height: 18,
                        lines: alloc::vec![LayoutLine {
                            spans: spans_with_bullet,
                            is_code: false,
                            is_heading: false,
                            scale: 1,
                            custom_color: None,
                        }],
                        bg_color: None,
                        border_color: None,
                        is_rule: false,
                        left_margin: indent_px,
                        image: None,
                        table_rows: None,
                        table_width: 0,
                        node_id: None,
                    };
                    curr_y += 18;
                    self.layout_boxes.push(box_item);
                }

                DocNode::Blockquote { lines } => {
                    let mut layout_lines = Vec::new();
                    for l in lines {
                        layout_lines.push(LayoutLine {
                            spans: alloc::vec![StyledSpan::plain(l)],
                            is_code: false,
                            is_heading: false,
                            scale: 1,
                            custom_color: Some(theme::TEXT_MUTED),
                        });
                    }

                    let height = (lines.len() as i32) * 16 + 8;
                    let box_item = LayoutBox {
                        y_offset: curr_y,
                        height,
                        lines: layout_lines,
                        bg_color: None,
                        border_color: Some(theme::ACCENT_BLUE),
                        is_rule: false,
                        left_margin: 16,
                        image: None,
                        table_rows: None,
                        table_width: 0,
                        node_id: None,
                    };

                    curr_y += height + 4;
                    self.layout_boxes.push(box_item);
                }

                DocNode::Rule => {
                    let box_item = LayoutBox {
                        y_offset: curr_y + 6,
                        height: 12,
                        lines: Vec::new(),
                        bg_color: None,
                        border_color: None,
                        is_rule: true,
                        left_margin: 0,
                        image: None,
                        table_rows: None,
                        table_width: 0,
                        node_id: None,
                    };
                    curr_y += 16;
                    self.layout_boxes.push(box_item);
                }

                DocNode::RawLines { lines } => {
                    let mut layout_lines = Vec::new();
                    for l in lines {
                        layout_lines.push(LayoutLine {
                            spans: alloc::vec![StyledSpan::plain(l)],
                            is_code: false,
                            is_heading: false,
                            scale: 1,
                            custom_color: Some(theme::TEXT_PRIMARY),
                        });
                    }

                    let height = (lines.len() as i32) * 14 + 10;
                    let box_item = LayoutBox {
                        y_offset: curr_y,
                        height,
                        lines: layout_lines,
                        bg_color: None,
                        border_color: None,
                        is_rule: false,
                        left_margin: 4,
                        image: None,
                        table_rows: None,
                        table_width: 0,
                        node_id: None,
                    };

                    curr_y += height;
                    self.layout_boxes.push(box_item);
                }

                DocNode::Image {
                    width,
                    height,
                    pixels,
                } => {
                    let box_h = (*height as i32) + 16;
                    let box_item = LayoutBox {
                        y_offset: curr_y,
                        height: box_h,
                        lines: Vec::new(),
                        bg_color: None,
                        border_color: None,
                        is_rule: false,
                        left_margin: 16,
                        image: Some((*width, *height, pixels.clone())),
                        table_rows: None,
                        table_width: 0,
                        node_id: None,
                    };
                    self.layout_boxes.push(box_item);
                }

                DocNode::Table { headers, rows } => {
                    let col_count = headers
                        .len()
                        .max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
                    if col_count == 0 {
                        continue;
                    }

                    let mut col_chars = alloc::vec![6usize; col_count];
                    for (i, h) in headers.iter().enumerate() {
                        if i < col_count {
                            col_chars[i] = col_chars[i].max(h.len());
                        }
                    }
                    for row in rows {
                        for (i, cell) in row.iter().enumerate() {
                            if i < col_count {
                                col_chars[i] = col_chars[i].max(cell.len());
                            }
                        }
                    }

                    let avail_w = content_width.saturating_sub(40);
                    let total_chars: usize = col_chars.iter().sum();
                    let mut col_widths = Vec::with_capacity(col_count);
                    for &chars in &col_chars {
                        let w = ((chars as f32 / total_chars.max(1) as f32) * (avail_w as f32))
                            .max(48.0) as u32;
                        col_widths.push(w);
                    }
                    let table_w: u32 = col_widths.iter().sum();

                    let row_h = 24u32;
                    let mut table_rows = Vec::new();
                    let mut rel_y = 0i32;

                    if !headers.is_empty() {
                        let mut cells = Vec::new();
                        let mut cell_x = 0i32;
                        for (i, w) in col_widths.iter().enumerate() {
                            let text = headers.get(i).cloned().unwrap_or_default();
                            cells.push(TableCellLayout {
                                text,
                                x: cell_x,
                                width: *w,
                            });
                            cell_x += *w as i32;
                        }
                        table_rows.push(TableRowLayout {
                            y_rel: rel_y,
                            height: row_h,
                            cells,
                            is_header: true,
                        });
                        rel_y += row_h as i32;
                    }

                    for row in rows {
                        let mut cells = Vec::new();
                        let mut cell_x = 0i32;
                        for (i, w) in col_widths.iter().enumerate() {
                            let text = row.get(i).cloned().unwrap_or_default();
                            cells.push(TableCellLayout {
                                text,
                                x: cell_x,
                                width: *w,
                            });
                            cell_x += *w as i32;
                        }
                        table_rows.push(TableRowLayout {
                            y_rel: rel_y,
                            height: row_h,
                            cells,
                            is_header: false,
                        });
                        rel_y += row_h as i32;
                    }

                    let box_h = rel_y + 12;
                    let box_item = LayoutBox {
                        y_offset: curr_y,
                        height: box_h,
                        lines: Vec::new(),
                        bg_color: None,
                        border_color: None,
                        is_rule: false,
                        left_margin: 16,
                        image: None,
                        table_rows: Some(table_rows),
                        table_width: table_w,
                        node_id: None,
                    };

                    curr_y += box_h + 8;
                    self.layout_boxes.push(box_item);
                }
                DocNode::Button { text, node_id } => {
                    let btn_text = alloc::format!("[ {} ]", text.trim());
                    let btn_width = ((btn_text.len() * 8) + 24) as i32;
                    let box_item = LayoutBox {
                        y_offset: curr_y,
                        height: 28,
                        lines: alloc::vec![LayoutLine {
                            spans: alloc::vec![StyledSpan {
                                text: btn_text,
                                bold: true,
                                italic: false,
                                code: true,
                                link: None,
                                color: Some(theme::TEXT_PRIMARY),
                            }],
                            is_code: true,
                            is_heading: false,
                            scale: 1,
                            custom_color: Some(theme::TEXT_PRIMARY),
                        }],
                        bg_color: Some(theme::ACCENT_BLUE),
                        border_color: Some(theme::BORDER),
                        is_rule: false,
                        left_margin: 8,
                        image: None,
                        table_rows: None,
                        table_width: btn_width as u32,
                        node_id: *node_id,
                    };
                    curr_y += 36;
                    self.layout_boxes.push(box_item);
                }
            }
        }

        self.total_height = curr_y + 32;
    }

    /// Render visible document boxes onto the surface given current scroll_y and viewport bounds.
    pub fn render_viewport(
        &self,
        surf: &mut ViSurface,
        view_x: i32,
        view_y: i32,
        view_w: u32,
        view_h: u32,
        scroll_y: i32,
    ) {
        let view_bottom = view_y + view_h as i32;

        for b in &self.layout_boxes {
            let screen_y = view_y + b.y_offset - scroll_y;
            let screen_bottom = screen_y + b.height;

            // Frustum / Culling check: skip boxes outside the viewport
            if screen_bottom <= view_y || screen_y >= view_bottom {
                continue;
            }

            // Draw background if specified (e.g. for code blocks)
            if let Some(bg) = b.bg_color {
                let draw_y = screen_y.max(view_y);
                let draw_h = (screen_bottom.min(view_bottom) - draw_y).max(0) as u32;
                let draw_w = view_w.saturating_sub((b.left_margin as u32) + 20);
                fill_rect(surf, view_x + b.left_margin, draw_y, draw_w, draw_h, bg);

                if let Some(border) = b.border_color {
                    stroke_rect(surf, view_x + b.left_margin, draw_y, draw_w, draw_h, border);
                }
            }

            // Draw horizontal rule
            if b.is_rule {
                let rule_y = screen_y + 4;
                if rule_y >= view_y && rule_y < view_bottom {
                    fill_rect(
                        surf,
                        view_x + 10,
                        rule_y,
                        view_w.saturating_sub(30),
                        1,
                        theme::BORDER,
                    );
                }
                continue;
            }

            // Draw image if present
            if let Some((iw, ih, image_pixels)) = &b.image {
                let img_y = screen_y + 8;
                if img_y + (*ih as i32) > view_y && img_y < view_bottom {
                    draw::draw_image(
                        surf,
                        view_x + b.left_margin,
                        img_y,
                        *iw,
                        *ih,
                        image_pixels,
                        (*iw * 4) as usize,
                    );
                }
                continue;
            }

            // Draw table if present
            if let Some(table_rows) = &b.table_rows {
                let tbl_x = view_x + b.left_margin;
                for row in table_rows {
                    let row_y = screen_y + row.y_rel;
                    if row_y + (row.height as i32) <= view_y || row_y >= view_bottom {
                        continue;
                    }

                    if row.is_header {
                        fill_rect(
                            surf,
                            tbl_x,
                            row_y,
                            b.table_width,
                            row.height,
                            theme::BG_INPUT,
                        );
                    }

                    fill_rect(
                        surf,
                        tbl_x,
                        row_y + row.height as i32 - 1,
                        b.table_width,
                        1,
                        theme::BORDER,
                    );

                    for cell in &row.cells {
                        let cell_screen_x = tbl_x + cell.x;
                        fill_rect(
                            surf,
                            cell_screen_x + cell.width as i32 - 1,
                            row_y,
                            1,
                            row.height,
                            theme::BORDER,
                        );

                        let color = if row.is_header {
                            theme::ACCENT_CYAN
                        } else {
                            theme::TEXT_PRIMARY
                        };
                        draw::draw_str(surf, cell_screen_x + 6, row_y + 6, &cell.text, color, 1);
                    }
                }
                stroke_rect(
                    surf,
                    tbl_x,
                    screen_y,
                    b.table_width,
                    (b.height - 12).max(1) as u32,
                    theme::BORDER,
                );
                continue;
            }

            // Draw lines of text
            let mut line_y = screen_y + 4;
            for line in &b.lines {
                let line_h = (8 * line.scale + 6) as i32;
                if line_y + line_h > view_y && line_y < view_bottom {
                    let mut cursor_x = view_x + 16 + b.left_margin;

                    for span in &line.spans {
                        let text_color = span
                            .color
                            .or(line.custom_color)
                            .unwrap_or(theme::TEXT_PRIMARY);

                        // Draw background pill for inline code
                        if span.code {
                            let span_w = (span.text.len() * 8) as u32 + 6;
                            fill_rect(
                                surf,
                                cursor_x - 2,
                                line_y - 1,
                                span_w,
                                8 * line.scale + 2,
                                theme::BG_INPUT,
                            );
                        }

                        draw::draw_str(surf, cursor_x, line_y, &span.text, text_color, line.scale);
                        cursor_x += (span.text.len() as i32) * (8 * line.scale as i32);
                    }
                }
                line_y += line_h;
            }
        }
    }

    /// Check if coordinates (click_x, click_y) hit any link span.
    pub fn hit_test_link(
        &self,
        click_x: i32,
        click_y: i32,
        view_x: i32,
        view_y: i32,
        scroll_y: i32,
    ) -> Option<String> {
        for b in &self.layout_boxes {
            let screen_y = view_y + b.y_offset - scroll_y;
            let screen_bottom = screen_y + b.height;

            if click_y < screen_y || click_y >= screen_bottom {
                continue;
            }

            let mut line_y = screen_y + 4;
            for line in &b.lines {
                let line_h = (8 * line.scale + 6) as i32;
                if click_y >= line_y && click_y < line_y + line_h {
                    let mut cursor_x = view_x + 16 + b.left_margin;
                    for span in &line.spans {
                        let span_w = (span.text.len() as i32) * (8 * line.scale as i32);
                        if click_x >= cursor_x && click_x < cursor_x + span_w {
                            if let Some(url) = &span.link {
                                return Some(url.clone());
                            }
                        }
                        cursor_x += span_w;
                    }
                }
                line_y += line_h;
            }
        }
        None
    }

    /// Check if coordinates (click_x, click_y) hit any layout box with a registered NodeId.
    pub fn hit_test_node(
        &self,
        click_x: i32,
        click_y: i32,
        view_x: i32,
        view_y: i32,
        scroll_y: i32,
    ) -> Option<dom_arena::NodeId> {
        for b in &self.layout_boxes {
            let screen_y = view_y + b.y_offset - scroll_y;
            let screen_bottom = screen_y + b.height;

            if click_y >= screen_y && click_y < screen_bottom {
                let box_x = view_x + 16 + b.left_margin;
                let box_w = if b.table_width > 0 {
                    b.table_width as i32
                } else {
                    b.lines
                        .iter()
                        .map(|l| {
                            l.spans
                                .iter()
                                .map(|s| (s.text.len() as i32) * (8 * l.scale as i32))
                                .sum::<i32>()
                        })
                        .max()
                        .unwrap_or(80)
                };

                if click_x >= box_x && click_x < box_x + box_w {
                    if let Some(id) = b.node_id {
                        return Some(id);
                    }
                }
            }
        }
        None
    }
}
