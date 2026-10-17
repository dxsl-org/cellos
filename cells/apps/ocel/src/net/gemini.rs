// SPDX-License-Identifier: MIT
//! Gemini documents use the cooperative transport in `http`; this module only
//! translates bounded Gemtext into native nodes.
extern crate alloc;
use alloc::{format, string::String, vec::Vec};
use crate::doc::{DocNode, StyledSpan};
use crate::draw::theme;

/// Parse Gemtext (text/gemini) into native CellOS DocNodes.
pub fn parse_gemtext(input: &str, base_url: &str) -> Vec<DocNode> {
    let mut nodes = Vec::new();
    let mut in_preformatted = false;
    let mut pre_alt = String::new();
    let mut pre_lines = Vec::new();

    for line in input.lines() {
        let trimmed = line.trim();

        // 1. Toggle preformatted mode ```
        if trimmed.starts_with("```") {
            if in_preformatted {
                nodes.push(DocNode::CodeBlock {
                    lang: core::mem::take(&mut pre_alt),
                    lines: core::mem::take(&mut pre_lines),
                });
                in_preformatted = false;
            } else {
                in_preformatted = true;
                pre_alt = String::from(trimmed.trim_start_matches("```").trim());
            }
            continue;
        }

        if in_preformatted {
            pre_lines.push(String::from(line));
            continue;
        }

        // 2. Empty line
        if trimmed.is_empty() {
            continue;
        }

        // 3. Headings: #, ##, ###
        if let Some(rest) = trimmed.strip_prefix("### ") {
            nodes.push(DocNode::Heading { level: 3, text: rest.trim().into() });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            nodes.push(DocNode::Heading { level: 2, text: rest.trim().into() });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            nodes.push(DocNode::Heading { level: 1, text: rest.trim().into() });
            continue;
        }

        // 4. Links: => URL [Optional label]
        if let Some(rest) = trimmed.strip_prefix("=>") {
            let link_content = rest.trim();
            let mut parts = link_content.split_whitespace();
            if let Some(raw_url) = parts.next() {
                let label: String = parts.collect::<Vec<&str>>().join(" ");
                let display_label = if label.is_empty() { raw_url } else { &label };
                let target_url = if raw_url.contains("://") {
                    String::from(raw_url)
                } else {
                    crate::resources::resolve(base_url, raw_url).unwrap_or_else(|_| String::from(raw_url))
                };
                let mut span = StyledSpan::plain(&format!("🚀 {}", display_label));
                span.link = Some(target_url);
                span.color = Some(theme::ACCENT_CYAN);
                span.bold = true;
                nodes.push(DocNode::Paragraph { spans: alloc::vec![span] });
            }
            continue;
        }

        // 5. Unordered List: * Item
        if let Some(rest) = trimmed.strip_prefix("* ") {
            nodes.push(DocNode::ListItem {
                bullet: '•',
                indent: 0,
                spans: alloc::vec![StyledSpan::plain(rest.trim())],
            });
            continue;
        }

        // 6. Blockquote: > Quote
        if let Some(rest) = trimmed.strip_prefix('>') {
            nodes.push(DocNode::Blockquote {
                lines: alloc::vec![String::from(rest.trim())],
            });
            continue;
        }

        // 7. Standard Text Paragraph
        nodes.push(DocNode::Paragraph {
            spans: alloc::vec![StyledSpan::plain(trimmed)],
        });
    }

    if in_preformatted && !pre_lines.is_empty() {
        nodes.push(DocNode::CodeBlock {
            lang: pre_alt,
            lines: pre_lines,
        });
    }

    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gemini_url() {
        let url = super::super::http::Url::parse("gemini://geminispace.info/search?q=1#title").unwrap();
        assert_eq!(url.host, "geminispace.info");
        assert_eq!(url.port, 1965);
        assert_eq!(url.path, "/search?q=1");
        assert_eq!(super::super::http::Url::parse("gemini://example.com:1966/page").unwrap().port, 1966);
        assert_eq!(super::super::http::Url::parse("gemini://rawtext.club").unwrap().path, "/");
    }

    #[test]
    fn test_parse_gemtext_elements() {
        let gemtext = "# Gemini Title\n## Subtitle\n* Bullet item 1\n* Bullet item 2\n> This is a quote from Geminispace\n=> /about About this capsule\n=> https://cellos.org CellOS Official Site\n```rust\nfn main() {}\n```\nFinal paragraph text.\n";
        let nodes = parse_gemtext(gemtext, "gemini://example.com/root");
        assert!(nodes.iter().any(|n| match n {
            DocNode::Heading { level, text } => *level == 1 && text == "Gemini Title",
            _ => false,
        }));
        assert!(nodes.iter().any(|n| match n {
            DocNode::ListItem { spans, .. } => spans.iter().any(|s| s.text == "Bullet item 1"),
            _ => false,
        }));
        assert!(nodes.iter().any(|n| match n {
            DocNode::Blockquote { lines } => lines.iter().any(|l| l == "This is a quote from Geminispace"),
            _ => false,
        }));
        assert!(nodes.iter().any(|n| match n {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.link == Some(String::from("https://cellos.org"))),
            _ => false,
        }));
        assert!(nodes.iter().any(|n| match n {
            DocNode::CodeBlock { lines, .. } => lines.iter().any(|l| l == "fn main() {}"),
            _ => false,
        }));
    }
}
