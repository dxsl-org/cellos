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
pub mod feed;
use crate::doc::DocNode;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DocFormat {
    Markdown,
    Html,
    PlainText,
    Source(syntax::Language),
    Feed,
}

impl DocFormat {
    pub fn detect_from_url_or_content(url: &str, content: &str) -> Self {
        let path = url.split(['?', '#']).next().unwrap_or(url);
        let lower = path.to_ascii_lowercase();
        let trimmed = content.trim_start();

        if lower.ends_with(".rss") || lower.ends_with("/rss") || lower.ends_with(".atom") || lower.ends_with("/feed")
            || ((lower.ends_with(".xml") || trimmed.starts_with("<?xml"))
                && (trimmed.contains("<rss") || trimmed.contains("<feed") || trimmed.contains("<channel")))
        {
            return Self::Feed;
        }

        if let Some(language) = syntax::Language::from_url(path) {
            return Self::Source(language);
        }
        if lower.ends_with(".json") || lower.ends_with(".jsonc") {
            return Self::Source(syntax::Language::Json);
        }
        if lower.ends_with(".xml") || lower.ends_with(".svg") {
            return Self::Source(syntax::Language::Xml);
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
        if ["<!doctype", "<html", "<head", "<body"].iter().any(|prefix| {
            trimmed.as_bytes().get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(prefix.as_bytes()))
        }) {
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
        DocFormat::Feed => (feed::parse_feed_to_nodes(content, base_url), Vec::new(), None),
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
    if matches!(format, DocFormat::Feed) {
        return Ok((feed::parse_feed_to_nodes(content, base_url), Vec::new(), None));
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

#[cfg(test)]
mod detection_tests {
    use super::DocFormat;

    #[test]
    fn extensionless_html_is_case_insensitive() {
        for prefix in ["<!doctype html>", "<!DOCTYPE HTML>", "<HtMl>", "<HEAD>", "<BoDy>"] {
            let content = alloc::format!(" \n{prefix}<p>Example Domain</p>");
            assert_eq!(DocFormat::detect_from_url_or_content("http://example.com/", &content), DocFormat::Html);
        }
        assert_eq!(DocFormat::detect_from_url_or_content("file:///source.txt", "<!doctype html>"), DocFormat::PlainText);
        assert_eq!(DocFormat::detect_from_url_or_content("file:///app.json", "{}"), DocFormat::Source(super::syntax::Language::Json));
        assert_eq!(DocFormat::detect_from_url_or_content("file:///tsconfig.jsonc", "{ /* comment */ }"), DocFormat::Source(super::syntax::Language::Json));
        assert_eq!(DocFormat::detect_from_url_or_content("file:///manifest.xml", "<manifest></manifest>"), DocFormat::Source(super::syntax::Language::Xml));
        assert_eq!(DocFormat::detect_from_url_or_content("https://news.ycombinator.com/rss", ""), DocFormat::Feed);
        assert_eq!(DocFormat::detect_from_url_or_content("https://blog.test/feed.atom", ""), DocFormat::Feed);
        assert_eq!(DocFormat::detect_from_url_or_content("https://news.test/feed.xml", "<rss version='2.0'><channel><title>News</title></channel></rss>"), DocFormat::Feed);
    }

    #[test]
    fn test_rss_and_atom_feed_parsing() {
        let rss_xml = r#"
            <rss version="2.0">
                <channel>
                    <title>CellOS Engineering Feed</title>
                    <description>Updates from the microkernel development team</description>
                    <link>https://cellos.org</link>
                    <item>
                        <title>Announcing CellOS G2 Architecture</title>
                        <link>https://cellos.org/posts/g2-launch</link>
                        <pubDate>Mon, 19 Oct 2026 09:00:00 GMT</pubDate>
                        <description>Formal memory isolation combined with single address space performance.</description>
                    </item>
                    <item>
                        <title>Universal Reader Engine Released</title>
                        <link>https://cellos.org/posts/reader-engine</link>
                        <pubDate>Sun, 18 Oct 2026 15:30:00 GMT</pubDate>
                        <description>Ocel gains Bionic Reading, RSS feed parsing and multi-page stitching.</description>
                    </item>
                </channel>
            </rss>
        "#;
        let nodes = super::feed::parse_feed_to_nodes(rss_xml, Some("https://cellos.org/"));
        assert!(nodes.iter().any(|n| matches!(n, super::DocNode::Heading { text, .. } if text.contains("CellOS Engineering Feed"))));
        assert!(nodes.iter().any(|n| matches!(n, super::DocNode::ListItem { spans, .. } if spans.iter().any(|s| s.text == "Announcing CellOS G2 Architecture" && s.link.as_deref() == Some("https://cellos.org/posts/g2-launch")))));
        assert!(nodes.iter().any(|n| matches!(n, super::DocNode::ListItem { spans, .. } if spans.iter().any(|s| s.text == "Universal Reader Engine Released"))));
        assert!(nodes.iter().any(|n| match n {
            super::DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("Formal memory isolation")),
            _ => false,
        }));

        let atom_xml = r#"
            <feed xmlns="http://www.w3.org/2005/Atom">
                <title>Hacker News Atom</title>
                <subtitle>Technology news feed</subtitle>
                <entry>
                    <title>Show HN: Ocel Fast Reader</title>
                    <link href="https://news.ycombinator.com/item?id=99999" />
                    <published>2026-10-19T10:00:00Z</published>
                    <summary>A native lightweight document and feed viewer.</summary>
                </entry>
            </feed>
        "#;
        let atom_nodes = super::feed::parse_feed_to_nodes(atom_xml, None);
        assert!(atom_nodes.iter().any(|n| matches!(n, super::DocNode::Heading { text, .. } if text.contains("Hacker News Atom"))));
        assert!(atom_nodes.iter().any(|n| matches!(n, super::DocNode::ListItem { spans, .. } if spans.iter().any(|s| s.text == "Show HN: Ocel Fast Reader" && s.link.as_deref() == Some("https://news.ycombinator.com/item?id=99999")))));
}
}
