// SPDX-License-Identifier: MIT
//! HTML subset parser & Tree Builder for Ocel.
//!
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use dom_arena::{DocumentArena, NodeData, NodeId};

use crate::doc::{DocNode, StyledSpan};
use crate::draw::theme;

pub struct HtmlOutput {
    pub nodes: Vec<DocNode>,
    pub scripts: Vec<String>,
    #[allow(dead_code)]
    pub arena: DocumentArena,
}

enum HtmlToken {
    StartTag {
        name: String,
        attributes: Vec<(String, String)>,
        self_closing: bool,
    },
    EndTag {
        name: String,
    },
    Text(String),
}

/// Tokenize an HTML string into tags and text chunks.
fn tokenize_html(input: &str) -> (Vec<HtmlToken>, Vec<String>) {
    let mut tokens = Vec::new();
    let mut scripts = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            // Check comment
            if chars.clone().take(3).collect::<String>() == "!--" {
                // Consume <!--
                for _ in 0..3 {
                    chars.next();
                }
                // Skip until -->
                while let Some(ch) = chars.next() {
                    if ch == '-' && chars.clone().take(2).collect::<String>() == "->" {
                        chars.next();
                        chars.next();
                        break;
                    }
                }
                continue;
            }

            // Read tag content
            let mut tag_content = String::new();
            while let Some(&ch) = chars.peek() {
                chars.next();
                if ch == '>' {
                    break;
                }
                tag_content.push(ch);
            }

            let trimmed_tag = tag_content.trim();
            if let Some(rest) = trimmed_tag.strip_prefix('/') {
                let name = rest.trim().to_ascii_lowercase();
                tokens.push(HtmlToken::EndTag { name });
            } else {
                let self_closing = trimmed_tag.ends_with('/');
                let tag_body = if self_closing {
                    trimmed_tag.trim_end_matches('/').trim()
                } else {
                    trimmed_tag
                };

                let mut parts = tag_body.split_whitespace();
                let name = parts.next().unwrap_or("").to_ascii_lowercase();
                let mut attributes = Vec::new();

                for part in parts {
                    if let Some((k, v)) = part.split_once('=') {
                        let clean_v = v.trim_matches('"').trim_matches('\'');
                        attributes.push((k.to_ascii_lowercase(), String::from(clean_v)));
                    } else if !part.is_empty() {
                        attributes.push((part.to_ascii_lowercase(), String::new()));
                    }
                }

                // If this is a <script> tag, capture the script body directly
                if name == "script" && !self_closing {
                    let mut script_body = String::new();
                    while let Some(sc) = chars.next() {
                        if sc == '<'
                            && chars
                                .clone()
                                .take(8)
                                .collect::<String>()
                                .eq_ignore_ascii_case("/script>")
                        {
                            for _ in 0..8 {
                                chars.next();
                            }
                            break;
                        }
                        script_body.push(sc);
                    }
                    scripts.push(script_body);
                    continue;
                }

                tokens.push(HtmlToken::StartTag {
                    name,
                    attributes,
                    self_closing,
                });
            }
        } else {
            // Text chunk
            let mut text = String::new();
            text.push(c);
            while let Some(&ch) = chars.peek() {
                if ch == '<' {
                    break;
                }
                text.push(ch);
                chars.next();
            }
            let decoded = decode_entities(&text);
            if !decoded.trim().is_empty() {
                tokens.push(HtmlToken::Text(decoded));
            }
        }
    }

    (tokens, scripts)
}

/// Parse HTML text into a `DocumentArena` and renderable `DocNode`s.
pub fn parse_html(input: &str) -> HtmlOutput {
    let (tokens, scripts) = tokenize_html(input);
    let mut arena = DocumentArena::new();
    let mut tag_stack: Vec<(String, NodeId)> = alloc::vec![(String::from("root"), arena.root)];

    for tok in tokens {
        match tok {
            HtmlToken::StartTag {
                name,
                attributes,
                self_closing,
            } => {
                let parent_id = tag_stack.last().map(|(_, id)| *id).unwrap_or(arena.root);
                let node_id = arena.alloc_node(NodeData::Element {
                    tag: name.clone(),
                    attributes,
                });
                arena.append_child(parent_id, node_id);

                let is_void_tag = self_closing
                    || matches!(
                        name.as_str(),
                        "hr" | "br" | "img" | "input" | "meta" | "link"
                    );

                if !is_void_tag {
                    tag_stack.push((name, node_id));
                }
            }
            HtmlToken::EndTag { name } => {
                if let Some(pos) = tag_stack.iter().rposition(|(t, _)| t == &name) {
                    tag_stack.truncate(pos);
                }
            }
            HtmlToken::Text(text) => {
                let parent_id = tag_stack.last().map(|(_, id)| *id).unwrap_or(arena.root);
                let text_id = arena.alloc_node(NodeData::Text(text));
                arena.append_child(parent_id, text_id);
            }
        }
    }

    let nodes = arena_to_doc_nodes(&arena);
    HtmlOutput {
        nodes,
        scripts,
        arena,
    }
}

/// Converts a `DocumentArena` DOM tree into renderable `DocNode`s for the layout engine.
pub fn arena_to_doc_nodes(arena: &DocumentArena) -> Vec<DocNode> {
    let mut doc_nodes = Vec::new();
    render_children(arena, arena.root, &mut doc_nodes);
    doc_nodes
}

fn render_children(arena: &DocumentArena, parent_id: NodeId, out: &mut Vec<DocNode>) {
    let Some(parent) = arena.get(parent_id) else {
        return;
    };
    let mut cur = parent.first_child;

    while let Some(child_id) = cur {
        if let Some(child) = arena.get(child_id) {
            match &child.data {
                NodeData::Element { tag, attributes: _ } => {
                    match tag.as_str() {
                        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                            let level = tag[1..].parse::<u8>().unwrap_or(1);
                            let text = arena.get_text_content(child_id);
                            out.push(DocNode::Heading {
                                level: level.clamp(1, 4),
                                text: text.trim().into(),
                            });
                        }
                        "p" | "div" | "section" | "article" | "header" | "footer" | "main" => {
                            let mut spans = Vec::new();
                            collect_inline_spans(
                                arena, child_id, &mut spans, false, false, false, None,
                            );
                            if !spans.is_empty() {
                                out.push(DocNode::Paragraph { spans });
                            }
                        }
                        "pre" => {
                            let raw_text = arena.get_text_content(child_id);
                            let lines: Vec<String> = raw_text.lines().map(String::from).collect();
                            out.push(DocNode::CodeBlock {
                                lang: String::new(),
                                lines,
                            });
                        }
                        "ul" | "ol" => {
                            let mut li_cur = child.first_child;
                            while let Some(li_id) = li_cur {
                                if let Some(li_node) = arena.get(li_id) {
                                    if li_node.tag() == Some("li") {
                                        let mut spans = Vec::new();
                                        collect_inline_spans(
                                            arena, li_id, &mut spans, false, false, false, None,
                                        );
                                        out.push(DocNode::ListItem {
                                            bullet: '•',
                                            indent: 0,
                                            spans,
                                        });
                                    }
                                }
                                li_cur = arena.get(li_id).and_then(|n| n.next_sibling);
                            }
                        }
                        "table" => {
                            let (headers, rows) = parse_table_node(arena, child_id);
                            if !headers.is_empty() || !rows.is_empty() {
                                out.push(DocNode::Table { headers, rows });
                            }
                        }
                        "hr" => {
                            out.push(DocNode::Rule);
                        }
                        "button" => {
                            let btn_text = arena.get_text_content(child_id);
                            out.push(DocNode::Button {
                                text: btn_text.trim().into(),
                                node_id: Some(child_id),
                            });
                        }
                        _ => {
                            // Recursively render other elements
                            render_children(arena, child_id, out);
                        }
                    }
                }
                NodeData::Text(text) => {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        out.push(DocNode::Paragraph {
                            spans: alloc::vec![StyledSpan::plain(trimmed)],
                        });
                    }
                }
                _ => {}
            }
        }
        cur = arena.get(child_id).and_then(|n| n.next_sibling);
    }
}

/// Recursively collects styled spans with inheritance of bold, italic, code, and link properties.
fn collect_inline_spans(
    arena: &DocumentArena,
    node_id: NodeId,
    out: &mut Vec<StyledSpan>,
    bold: bool,
    italic: bool,
    code: bool,
    link: Option<String>,
) {
    let Some(node) = arena.get(node_id) else {
        return;
    };
    let mut cur = node.first_child;

    while let Some(child_id) = cur {
        if let Some(child) = arena.get(child_id) {
            match &child.data {
                NodeData::Element { tag, attributes } => {
                    let next_bold = bold || tag == "b" || tag == "strong";
                    let next_italic = italic || tag == "i" || tag == "em";
                    let next_code = code || tag == "code";
                    let next_link = if tag == "a" {
                        attributes
                            .iter()
                            .find(|(k, _)| k == "href")
                            .map(|(_, v)| v.clone())
                            .or(link.clone())
                    } else {
                        link.clone()
                    };

                    collect_inline_spans(
                        arena,
                        child_id,
                        out,
                        next_bold,
                        next_italic,
                        next_code,
                        next_link,
                    );
                }
                NodeData::Text(text) if !text.is_empty() => {
                    let color = if link.is_some() {
                        Some(theme::ACCENT_BLUE)
                    } else if code {
                        Some(theme::ACCENT_CYAN)
                    } else {
                        None
                    };

                    out.push(StyledSpan {
                        text: text.clone(),
                        bold,
                        italic,
                        code,
                        link: link.clone(),
                        color,
                    });
                }
                _ => {}
            }
        }
        cur = arena.get(child_id).and_then(|n| n.next_sibling);
    }
}

/// Extracts table headers and rows from a `<table>` node.
fn parse_table_node(arena: &DocumentArena, table_id: NodeId) -> (Vec<String>, Vec<Vec<String>>) {
    let mut headers = Vec::new();
    let mut rows = Vec::new();

    let Some(table) = arena.get(table_id) else {
        return (headers, rows);
    };

    let mut cur = table.first_child;
    while let Some(tr_id) = cur {
        if let Some(tr_node) = arena.get(tr_id) {
            let tag = tr_node.tag().unwrap_or("");
            if tag == "tr" {
                let mut cell_cur = tr_node.first_child;
                let mut row_cells = Vec::new();
                let mut is_header_row = false;

                while let Some(cell_id) = cell_cur {
                    if let Some(cell_node) = arena.get(cell_id) {
                        let cell_tag = cell_node.tag().unwrap_or("");
                        if cell_tag == "th" {
                            is_header_row = true;
                            row_cells.push(arena.get_text_content(cell_id).trim().into());
                        } else if cell_tag == "td" {
                            row_cells.push(arena.get_text_content(cell_id).trim().into());
                        }
                    }
                    cell_cur = arena.get(cell_id).and_then(|n| n.next_sibling);
                }

                if is_header_row && headers.is_empty() {
                    headers = row_cells;
                } else if !row_cells.is_empty() {
                    rows.push(row_cells);
                }
            } else if tag == "tbody" || tag == "thead" {
                // Nested tbody/thead
                let (sub_h, sub_r) = parse_table_node(arena, tr_id);
                if headers.is_empty() && !sub_h.is_empty() {
                    headers = sub_h;
                }
                rows.extend(sub_r);
            }
        }
        cur = arena.get(tr_id).and_then(|n| n.next_sibling);
    }

    (headers, rows)
}

/// Decode basic HTML entities into UTF-8 characters.
pub fn decode_entities(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

/// Strip all `<...>` tags and decode basic HTML entities (utility helper).
#[allow(dead_code)]
pub fn strip_tags(input: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;

    for c in input.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(c);
        }
    }

    decode_entities(&out)
}
