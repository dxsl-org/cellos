// SPDX-License-Identifier: MIT
//! Readability content extractor for Ocel — clean, distraction-free article reading.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use dom_arena::{DocumentArena, NodeData, NodeId};

use crate::doc::{DocNode, StyledSpan};

pub struct ArticleExtract {
    #[allow(dead_code)]
    pub title: String,
    pub nodes: Vec<DocNode>,
}

/// Extract main article content from DOM tree.
#[cfg(test)]
pub fn extract_article(arena: &DocumentArena, base_url: &str, fallback_title: &str) -> ArticleExtract {
    extract_article_cached(arena, base_url, fallback_title, &[])
}

pub fn extract_article_cached(arena: &DocumentArena, _base_url: &str, fallback_title: &str, assets: &[DocNode]) -> ArticleExtract {
    let mut best_node = None;
    let mut highest_score = -1000i32;

    // 1. Traverse all element nodes and score them
    for node in &arena.nodes {
        if let NodeData::Element { tag, attributes } = &node.data {
            let tag_name = tag.as_str();

            // Disqualify noise tags
            if matches!(
                tag_name,
                "nav" | "footer" | "header" | "aside" | "script" | "style" | "noscript" | "form"
            ) {
                continue;
            }

            let mut score = 0i32;

            // Tag bonuses
            match tag_name {
                "article" => score += 40,
                "main" => score += 30,
                "section" => score += 15,
                "div" => score += 5,
                _ => {}
            }

            // Attribute scoring (class, id)
            for (key, val) in attributes {
                let lower_val = val.to_ascii_lowercase();
                if key == "class" || key == "id" {
                    if lower_val.contains("article")
                        || lower_val.contains("content")
                        || lower_val.contains("post")
                        || lower_val.contains("entry")
                        || lower_val.contains("story")
                        || lower_val.contains("body")
                    {
                        score += 25;
                    }
                    if lower_val.contains("comment")
                        || lower_val.contains("sidebar")
                        || lower_val.contains("nav")
                        || lower_val.contains("footer")
                        || lower_val.contains("widget")
                        || lower_val.contains("banner")
                        || lower_val.contains("sponsor")
                        || lower_val.contains("ad")
                    {
                        score -= 30;
                    }
                }
            }

            // Paragraph child density
            let mut p_cur = node.first_child;
            let mut p_count = 0;
            while let Some(pid) = p_cur {
                if let Some(p_node) = arena.get(pid) {
                    if p_node.tag() == Some("p") {
                        let text_len = arena.get_text_content(pid).trim().len();
                        if text_len > 30 {
                            score += (text_len / 20) as i32;
                            p_count += 1;
                        }
                    }
                }
                p_cur = arena.get(pid).and_then(|n| n.next_sibling);
            }

            if p_count > 0 && score > highest_score {
                highest_score = score;
                best_node = Some(node.id);
            }
        }
    }

    let target_node = best_node.unwrap_or(arena.root);

    // Extract title (h1 or fallback_title)
    let mut article_title = String::new();
    let mut cur = arena.get(target_node).and_then(|n| n.first_child);
    while let Some(cid) = cur {
        if let Some(child) = arena.get(cid) {
            if child.tag() == Some("h1") {
                article_title = arena.get_text_content(cid).trim().into();
                break;
            }
        }
        cur = arena.get(cid).and_then(|n| n.next_sibling);
    }
    if article_title.is_empty() {
        article_title = String::from(fallback_title);
    }

    // Render clean article nodes
    let mut nodes = Vec::new();

    // Extract first, then add a fallback title only when it is absent.
    render_clean_subtree(arena, target_node, &mut nodes, assets);
    if let Some(title)=nodes.iter().find_map(|node|if let DocNode::Heading{level:1,text}=node{Some(text)}else{None}) {
        article_title=title.clone();
    }else{
        nodes.insert(0, DocNode::Heading { level:1, text:article_title.clone() });
    }

    ArticleExtract {
        title: format!("{} [Reader]", article_title),
        nodes,
    }
}

fn render_clean_subtree(
    arena: &DocumentArena,
    parent_id: NodeId,
    out: &mut Vec<DocNode>,
    assets: &[DocNode],
) {
    let Some(parent) = arena.get(parent_id) else {
        return;
    };
    let mut cur = parent.first_child;

    while let Some(child_id) = cur {
        if let Some(child) = arena.get(child_id) {
            match &child.data {
                NodeData::Element { tag, attributes } => {
                    let tag_name = tag.as_str();

                    // Skip noisy tags
                    if matches!(
                        tag_name,
                        "script" | "style" | "nav" | "footer" | "header" | "aside" | "form" | "noscript"
                    ) {
                        cur = child.next_sibling;
                        continue;
                    }

                    match tag_name {
                        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                            let level = tag[1..].parse::<u8>().unwrap_or(2);
                            let text = String::from(arena.get_text_content(child_id).trim());
                            if !text.is_empty() {
                                out.push(DocNode::Heading {
                                    level: level.clamp(1, 4),
                                    text,
                                });
                            }
                        }
                        "p" => {
                            let text = String::from(arena.get_text_content(child_id).trim());
                            if !text.is_empty() {
                                out.push(DocNode::Paragraph {
                                    spans: alloc::vec![StyledSpan::plain(&text)],
                                });
                            }
                            append_cached_images(arena,child_id,out,assets);
                        }
                        "pre" | "code" => {
                            let text = arena.get_text_content(child_id);
                            let lines = text.lines().map(String::from).collect();
                            out.push(DocNode::CodeBlock {
                                lang: String::new(),
                                lines,
                            });
                        }
                        "blockquote" => {
                            let text = arena.get_text_content(child_id);
                            let lines = text.lines().map(String::from).collect();
                            out.push(DocNode::Blockquote { lines });
                        }
                        "img" => {
                            let alt = attributes.iter().find(|(k, _)| k == "alt").map(|(_, v)| v.as_str()).unwrap_or("Image");
                            if let Some(image)=assets.iter().find(|node|matches!(node,DocNode::DomImage{node_id,..} if *node_id==child_id)) {
                                out.push(image.clone());
                                cur=child.next_sibling;continue;
                            }
                            let mut span = StyledSpan::plain(&format!("[🖼 {}]", alt));
                            span.color = Some(crate::draw::theme::TEXT_MUTED);
                            out.push(DocNode::Paragraph { spans: alloc::vec![span] });
                        }
                        "ul" | "ol" => {
                            let mut li_cur = child.first_child;
                            while let Some(li_id) = li_cur {
                                if let Some(li_node) = arena.get(li_id) {
                                    if li_node.tag() == Some("li") {
                                        let text = String::from(arena.get_text_content(li_id).trim());
                                        out.push(DocNode::ListItem {
                                            bullet: '•',
                                            indent: 0,
                                            spans: alloc::vec![StyledSpan::plain(&text)],
                                        });
                                    }
                                }
                                li_cur = arena.get(li_id).and_then(|n| n.next_sibling);
                            }
                        }
                        _ => {
                            // Recursively extract from div, section, etc.
                            render_clean_subtree(arena, child_id, out, assets);
                        }
                    }
                }
                NodeData::Text(text) => {
                    let trimmed = text.trim();
                    if trimmed.len() > 40 {
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_readability_extraction() {
        let html = r#"
            <html>
            <body>
                <nav><a href="/">Home</a><a href="/news">News</a></nav>
                <aside class="sidebar">Ads and widgets</aside>
                <article class="post-content">
                    <h1>CellOS Revolutionizes Microkernels</h1>
                    <p>CellOS introduces a single address space capability architecture that enables blazing fast inter-cell communication while preserving complete memory isolation.</p>
                    <p>Unlike traditional monolithic kernels, CellOS isolates workloads in native domain cells with bounded footprints.</p>
                </article>
                <footer>Copyright 2026 CellOS Foundation</footer>
            </body>
            </html>
        "#;
        let parsed = crate::parser::html::parse_html(html);
        let extracted = extract_article(&parsed.arena, "https://cellos.org/blog/1", "CellOS Blog");

        assert!(extracted.title.contains("CellOS Revolutionizes Microkernels"));
        // The navigation and footer should be stripped!
        assert!(!extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("Copyright 2026")),
            _ => false,
        }));
        // The main content should be extracted!
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("CellOS introduces a single address space")),
            _ => false,
        }));
    }
}

fn append_cached_images(arena:&DocumentArena,id:NodeId,out:&mut Vec<DocNode>,assets:&[DocNode]) {
    let Some(node)=arena.get(id)else{return;};
    if node.tag()==Some("img") {
        if let Some(image)=assets.iter().find(|asset|matches!(asset,DocNode::DomImage{node_id,..} if *node_id==id)){out.push(image.clone());}
        return;
    }
    let mut child=node.first_child;
    while let Some(id)=child {append_cached_images(arena,id,out,assets);child=arena.get(id).and_then(|node|node.next_sibling);}
}
