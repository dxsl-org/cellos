// SPDX-License-Identifier: MIT
//! Markdown parser for Ocel.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::doc::{DocNode, StyledSpan};

pub fn parse_markdown(input: &str) -> Vec<DocNode> {
    let mut nodes = Vec::new();
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_lines = Vec::new();

    let mut in_table = false;
    let mut table_headers: Vec<String> = Vec::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    for line in input.lines() {
        let trimmed = line.trim();

        // 1. Check code block fences
        if trimmed.starts_with("```") {
            if in_code_block {
                nodes.push(DocNode::CodeBlock {
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

        // Table processing
        if trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 1 {
            let cells = split_table_cells(trimmed);
            if !in_table {
                table_headers = cells;
                in_table = true;
            } else {
                let is_separator = cells.iter().all(|c| {
                    c.chars()
                        .all(|ch| ch == '-' || ch == ':' || ch.is_whitespace())
                });
                if !is_separator {
                    table_rows.push(cells);
                }
            }
            continue;
        } else if in_table {
            nodes.push(DocNode::Table {
                headers: core::mem::take(&mut table_headers),
                rows: core::mem::take(&mut table_rows),
            });
            in_table = false;
        }

        // 2. Empty line -> spacer or paragraph break
        if trimmed.is_empty() {
            continue;
        }

        // 3. Horizontal Rule
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            nodes.push(DocNode::Rule);
            continue;
        }

        // 4. Headings
        if let Some(rest) = trimmed.strip_prefix("# ") {
            nodes.push(DocNode::Heading {
                level: 1,
                text: String::from(rest.trim()),
            });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            nodes.push(DocNode::Heading {
                level: 2,
                text: String::from(rest.trim()),
            });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("### ") {
            nodes.push(DocNode::Heading {
                level: 3,
                text: String::from(rest.trim()),
            });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("#### ") {
            nodes.push(DocNode::Heading {
                level: 4,
                text: String::from(rest.trim()),
            });
            continue;
        }

        // 5. Blockquote
        if let Some(rest) = trimmed.strip_prefix("> ") {
            nodes.push(DocNode::Blockquote {
                lines: alloc::vec![String::from(rest)],
            });
            continue;
        }

        // 6. List items
        let indent = (line.len() - line.trim_start().len()) as u8 / 2;
        if let Some(rest) = trimmed.strip_prefix("- ") {
            nodes.push(DocNode::ListItem {
                bullet: '*',
                indent,
                spans: parse_inlines(rest),
            });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("* ") {
            nodes.push(DocNode::ListItem {
                bullet: '*',
                indent,
                spans: parse_inlines(rest),
            });
            continue;
        }

        // 7. Standard Paragraph
        nodes.push(DocNode::Paragraph {
            spans: parse_inlines(trimmed),
        });
    }

    if in_table && !table_headers.is_empty() {
        nodes.push(DocNode::Table {
            headers: table_headers,
            rows: table_rows,
        });
    }

    if in_code_block && !code_lines.is_empty() {
        nodes.push(DocNode::CodeBlock {
            lang: code_lang,
            lines: code_lines,
        });
    }

    nodes
}

fn split_table_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim().trim_start_matches('|').trim_end_matches('|');
    trimmed.split('|').map(|c| String::from(c.trim())).collect()
}

/// Parse inline markdown: `code`, **bold**, [link](url)
pub fn parse_inlines(text: &str) -> Vec<StyledSpan> {
    let mut spans = Vec::new();
    let mut chars = text.chars().peekable();
    let mut buf = String::new();

    while let Some(c) = chars.next() {
        match c {
            '`' => {
                if !buf.is_empty() {
                    spans.push(StyledSpan::plain(&buf));
                    buf.clear();
                }
                let mut code_buf = String::new();
                for inner in chars.by_ref() {
                    if inner == '`' {
                        break;
                    }
                    code_buf.push(inner);
                }
                spans.push(StyledSpan::code(&code_buf));
            }
            '*' if chars.peek() == Some(&'*') => {
                chars.next(); // consume second '*'
                if !buf.is_empty() {
                    spans.push(StyledSpan::plain(&buf));
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
                let mut s = StyledSpan::plain(&bold_buf);
                s.bold = true;
                spans.push(s);
            }
            '[' => {
                if !buf.is_empty() {
                    spans.push(StyledSpan::plain(&buf));
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

                let mut s = StyledSpan::plain(&link_text);
                s.link = Some(link_url);
                s.color = Some(crate::draw::theme::ACCENT_BLUE);
                spans.push(s);
            }
            _ => {
                buf.push(c);
            }
        }
    }

    if !buf.is_empty() {
        spans.push(StyledSpan::plain(&buf));
    }

    spans
}
