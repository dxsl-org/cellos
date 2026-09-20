// SPDX-License-Identifier: MIT
//! Content parsers and auto-detection for Ocel.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

pub mod html;
pub mod markdown;

use crate::doc::DocNode;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DocFormat {
    Markdown,
    Html,
    PlainText,
}

impl DocFormat {
    pub fn detect_from_url_or_content(url: &str, content: &str) -> Self {
        let lower = url.to_ascii_lowercase();

        if lower.ends_with(".md") || lower.ends_with(".markdown") {
            return Self::Markdown;
        }
        if lower.ends_with(".html") || lower.ends_with(".htm") {
            return Self::Html;
        }
        if lower.ends_with(".txt")
            || lower.ends_with(".log")
            || lower.ends_with(".rs")
            || lower.ends_with(".c")
            || lower.ends_with(".h")
            || lower.ends_with(".json")
            || lower.ends_with(".toml")
        {
            return Self::PlainText;
        }

        // Content sniffing if extension is ambiguous or missing
        let trimmed = content.trim_start();
        if trimmed.starts_with("<!DOCTYPE")
            || trimmed.starts_with("<html")
            || trimmed.starts_with("<head")
            || trimmed.starts_with("<body")
        {
            return Self::Html;
        }

        if trimmed.starts_with("# ") || trimmed.starts_with("## ") || trimmed.contains("\n# ") {
            return Self::Markdown;
        }

        Self::PlainText
    }
}

pub fn parse_content(
    format: DocFormat,
    content: &str,
) -> (Vec<DocNode>, Vec<String>, Option<dom_arena::DocumentArena>) {
    match format {
        DocFormat::Markdown => (markdown::parse_markdown(content), Vec::new(), None),
        DocFormat::Html => {
            let res = html::parse_html(content);
            (res.nodes, res.scripts, Some(res.arena))
        }
        DocFormat::PlainText => {
            let mut lines = Vec::new();
            for l in content.lines() {
                lines.push(String::from(l));
            }
            (alloc::vec![DocNode::RawLines { lines }], Vec::new(), None)
        }
    }
}
