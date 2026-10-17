// SPDX-License-Identifier: MIT
//! Content parsers and auto-detection for Ocel.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

pub mod html;
pub mod markdown;
pub mod syntax;
pub mod css;
pub mod web;
pub mod readability;

use crate::doc::DocNode;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DocFormat {
    Markdown,
    Html,
    PlainText,
    Source(syntax::Language),
}

impl DocFormat {
    pub fn detect_from_url_or_content(url: &str, content: &str) -> Self {
        let path = url.split(['?', '#']).next().unwrap_or(url);
        let lower = path.to_ascii_lowercase();

        if let Some(language) = syntax::Language::from_url(path) {
            return Self::Source(language);
        }
        if lower.ends_with(".md") || lower.ends_with(".markdown") {
            return Self::Markdown;
        }
        if lower.ends_with(".html") || lower.ends_with(".htm") {
            return Self::Html;
        }
        if lower.ends_with(".txt")
            || lower.ends_with(".log")
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

#[allow(dead_code)]
pub fn parse_content(
    format: DocFormat,
    content: &str,
) -> (Vec<DocNode>, Vec<html::ScriptSource>, Option<dom_arena::DocumentArena>) {
    parse_content_with_base(format, content, None)
}

pub fn parse_content_with_base(
    format: DocFormat,
    content: &str,
    base_url: Option<&str>,
) -> (Vec<DocNode>, Vec<html::ScriptSource>, Option<dom_arena::DocumentArena>) {
    match format {
        DocFormat::Markdown => (markdown::parse_markdown_with_base(content, base_url), Vec::new(), None),
        DocFormat::Html => {
            let res = html::parse_html_with_base(content, base_url);
            (res.nodes, res.scripts, Some(res.arena))
        }
        DocFormat::Source(language) => (
            alloc::vec![DocNode::CodeBlock {
                lang: String::from(language.name()),
                lines: content.lines().map(String::from).collect(),
            }],
            Vec::new(),
            None,
        ),
        DocFormat::PlainText => {
            let mut lines = Vec::new();
            for l in content.lines() {
                lines.push(String::from(l));
            }
            (alloc::vec![DocNode::RawLines { lines }], Vec::new(), None)
        }
    }
}

pub fn parse_content_with_base_checked(
    format: DocFormat, content: &str, base_url: Option<&str>,
) -> Result<(Vec<DocNode>, Vec<html::ScriptSource>, Option<dom_arena::DocumentArena>), String> {
    crate::budget::validate_text(content)?;
    if matches!(format, DocFormat::Html) {
        let output = html::parse_html_with_base_checked(content, base_url)?;
        return Ok((output.nodes, output.scripts, Some(output.arena)));
    }
    if content.lines().count() > crate::budget::MAX_DOM_NODES {
        return Err(String::from("Document exceeds node/line budget"));
    }
    if matches!(format, DocFormat::Markdown) {
        crate::budget::validate_markdown(content)?;
        let mut images = Vec::new();
        let mut fenced=false;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("```"){fenced=!fenced;continue;}
            if fenced{continue;}
            if line.starts_with("![") && line.ends_with(')') {
                if let Some(end) = line.find("](") { images.push(&line[end + 2..line.len() - 1]); }
            }
        }
        validate_images(base_url, images.into_iter())?;
    }
    Ok(parse_content_with_base(format, content, base_url))
}

fn validate_images<'a>(base: Option<&str>, urls: impl Iterator<Item=&'a str>) -> Result<(), String> {
    let mut count = 0usize; let mut bytes = 0usize;
    for url in urls {
        count += 1;
        if count > crate::budget::MAX_IMAGES { return Err(String::from("Document exceeds image count budget")); }
        if let Some(base) = base {
            if let Ok(data) = crate::resources::load_binary(base, url) {
                match crate::budget::image_bytes(&data) {
                    Ok(size) => {
                        bytes = bytes.checked_add(size).ok_or_else(|| String::from("Image budget overflow"))?;
                        if bytes > crate::budget::MAX_IMAGE_TOTAL_BYTES { return Err(String::from("Document exceeds decoded image byte budget")); }
                    }
                    Err(error) if error.contains("budget") => return Err(error),
                    Err(_) => {}
                }
            }
        }
    }
    Ok(())
}
