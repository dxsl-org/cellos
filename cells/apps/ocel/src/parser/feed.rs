// SPDX-License-Identifier: MIT
//! RSS and Atom XML feed parser for Ocel.

extern crate alloc;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::doc::{DocNode, StyledSpan};

pub struct FeedItem {
    pub title: String,
    pub link: String,
    pub date: Option<String>,
    pub summary: Option<String>,
}

pub struct FeedData {
    pub title: String,
    pub description: Option<String>,
    pub items: Vec<FeedItem>,
}

fn unescape_xml(input: &str) -> String {
    let mut s = input.trim();
    if s.starts_with("<![CDATA[") && s.ends_with("]]>") {
        s = &s[9..s.len() - 3];
    }
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .trim()
        .to_string()
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open_tag = format!("<{}", tag);
    let mut pos = 0;
    while let Some(start) = xml[pos..].find(&open_tag) {
        let abs_start = pos + start;
        let after_open = &xml[abs_start + open_tag.len()..];
        if let Some(close_bracket) = after_open.find('>') {
            let full_open_end = abs_start + open_tag.len() + close_bracket + 1;
            if close_bracket > 0 && after_open.as_bytes()[close_bracket - 1] == b'/' {
                if let Some(href_pos) = after_open[..close_bracket].find("href=\"") {
                    let href_val = &after_open[..close_bracket][href_pos + 6..];
                    if let Some(quote_end) = href_val.find('"') {
                        return Some(unescape_xml(&href_val[..quote_end]));
                    }
                }
                return None;
            }
            let close_tag = format!("</{}>", tag);
            if let Some(end_pos) = xml[full_open_end..].find(&close_tag) {
                let inner = &xml[full_open_end..full_open_end + end_pos];
                return Some(unescape_xml(inner));
            }
        }
        pos = abs_start + open_tag.len();
    }
    None
}

pub fn parse_feed_to_nodes(content: &str, base_url: Option<&str>) -> Vec<DocNode> {
    let feed = parse_feed(content, base_url);
    let mut nodes = Vec::new();

    // 1. Feed Title
    nodes.push(DocNode::Heading {
        level: 1,
        text: format!("{} [RSS/Feed]", feed.title),
    });

    // 2. Feed Description
    if let Some(desc) = feed.description {
        if !desc.is_empty() {
            let mut span = StyledSpan::plain(&desc);
            span.italic = true;
            span.color = Some(crate::draw::theme::TEXT_MUTED);
            nodes.push(DocNode::Paragraph {
                spans: alloc::vec![span],
            });
        }
    }

    nodes.push(DocNode::Rule);

    // 3. Feed Items
    for item in feed.items {
        let mut title_span = StyledSpan::plain(&item.title);
        title_span.link = Some(item.link);
        title_span.bold = true;
        title_span.color = Some(crate::draw::theme::ACCENT_CYAN);
        nodes.push(DocNode::ListItem {
            bullet: '•',
            indent: 0,
            spans: alloc::vec![title_span],
        });

        if let Some(date) = item.date {
            let mut date_span = StyledSpan::plain(&format!("  📅 {}", date));
            date_span.italic = true;
            date_span.color = Some(crate::draw::theme::TEXT_MUTED);
            nodes.push(DocNode::Paragraph {
                spans: alloc::vec![date_span],
            });
        }

        if let Some(summary) = item.summary {
            let mut sum_span = StyledSpan::plain(&format!("  {}", summary));
            sum_span.color = Some(crate::draw::theme::TEXT_PRIMARY);
            nodes.push(DocNode::Paragraph {
                spans: alloc::vec![sum_span],
            });
        }
    }

    nodes
}

pub fn parse_feed(content: &str, base_url: Option<&str>) -> FeedData {
    let base = base_url.unwrap_or("");
    let is_atom = content.contains("<feed");

    let feed_title = extract_xml_tag(content, "title").unwrap_or_else(|| String::from("News Feed"));
    let feed_desc = if is_atom {
        extract_xml_tag(content, "subtitle")
    } else {
        extract_xml_tag(content, "description")
    };

    let mut items = Vec::new();
    let item_tag = if is_atom { "entry" } else { "item" };
    let open_pattern = format!("<{}", item_tag);
    let close_pattern = format!("</{}>", item_tag);

    let mut pos = 0;
    while let Some(start) = content[pos..].find(&open_pattern) {
        let abs_start = pos + start;
        let rest = &content[abs_start..];
        let Some(end) = rest.find(&close_pattern) else { break; };
        let item_xml = &rest[..end + close_pattern.len()];

        let title = extract_xml_tag(item_xml, "title").unwrap_or_else(|| String::from("Untitled Entry"));
        let link_raw = if is_atom {
            extract_atom_link(item_xml).or_else(|| extract_xml_tag(item_xml, "link"))
        } else {
            extract_xml_tag(item_xml, "link")
        }.unwrap_or_default();

        let link = if !link_raw.is_empty() && !base.is_empty() {
            crate::resources::resolve(base, &link_raw).unwrap_or(link_raw)
        } else {
            link_raw
        };

        let date = if is_atom {
            extract_xml_tag(item_xml, "published").or_else(|| extract_xml_tag(item_xml, "updated"))
        } else {
            extract_xml_tag(item_xml, "pubDate").or_else(|| extract_xml_tag(item_xml, "dc:date"))
        };

        let summary = if is_atom {
            extract_xml_tag(item_xml, "summary").or_else(|| extract_xml_tag(item_xml, "content"))
        } else {
            extract_xml_tag(item_xml, "description").or_else(|| extract_xml_tag(item_xml, "content:encoded"))
        };

        let clean_summary = summary.map(|s| strip_html_tags(&s));

        items.push(FeedItem {
            title,
            link,
            date,
            summary: clean_summary,
        });

        pos = abs_start + end + close_pattern.len();
        if items.len() >= 100 {
            break;
        }
    }

    FeedData {
        title: feed_title,
        description: feed_desc,
        items,
    }
}

fn extract_atom_link(item_xml: &str) -> Option<String> {
    let mut pos = 0;
    while let Some(start) = item_xml[pos..].find("<link") {
        let abs_start = pos + start;
        let rest = &item_xml[abs_start..];
        if let Some(close) = rest.find('>') {
            let tag_content = &rest[..close];
            if let Some(href_idx) = tag_content.find("href=\"") {
                let href_val = &tag_content[href_idx + 6..];
                if let Some(q_end) = href_val.find('"') {
                    return Some(unescape_xml(&href_val[..q_end]));
                }
            }
        }
        pos = abs_start + 5;
    }
    None
}

fn strip_html_tags(input: &str) -> String {
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
    out.split_whitespace().collect::<Vec<&str>>().join(" ")
}
