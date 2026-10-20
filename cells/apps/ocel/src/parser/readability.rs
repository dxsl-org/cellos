// SPDX-License-Identifier: MIT
//! Readability content extractor for Ocel — clean, distraction-free article reading.

extern crate alloc;
use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::{String, ToString};
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

pub fn extract_article_cached(arena: &DocumentArena, base_url: &str, fallback_title: &str, assets: &[DocNode]) -> ArticleExtract {
    let is_portal = is_portal_or_homepage(base_url);

    // 1. JSON-LD, Meta tags, Microdata, Semantic HTML5 Search & htmldate Cascade
    let (json_headline, json_sapo, json_author, json_date) = extract_json_ld_metadata(arena);
    let (meta_title, meta_sapo, meta_author, meta_date) = extract_meta_tags(arena);

    let mut schema_headline: Option<String> = json_headline.or(meta_title);
    let mut schema_sapo: Option<String> = json_sapo.or(meta_sapo);
    let mut schema_body_node: Option<NodeId> = None;
    let mut schema_author: Option<String> = json_author.or(meta_author);
    let mut schema_date: Option<String> = json_date.or(meta_date);

    for node in &arena.nodes {
        if is_noise_or_modal(arena, node.id) {
            continue;
        }
        if let NodeData::Element { tag, attributes } = &node.data {
            let mut itemprop: Option<&str> = None;
            let mut data_role: Option<&str> = None;
            let mut class_val: Option<&str> = None;
            for (k, v) in attributes {
                if k == "itemprop" {
                    itemprop = Some(v.as_str());
                } else if k == "data-role" {
                    data_role = Some(v.as_str());
                } else if k == "class" {
                    class_val = Some(v.as_str());
                }
            }

            // Headline
            if schema_headline.is_none() {
                if matches!(itemprop, Some("headline" | "name")) || matches!(data_role, Some("title")) {
                    let text = arena.get_text_content(node.id).trim().to_string();
                    if text.chars().count() > 3 {
                        schema_headline = Some(text);
                    }
                }
            }

            // Sapo / Description
            if schema_sapo.is_none() {
                if matches!(itemprop, Some("description")) || matches!(data_role, Some("sapo")) {
                    let text = arena.get_text_content(node.id).trim().to_string();
                    if text.chars().count() > 10 {
                        schema_sapo = Some(text);
                    }
                } else if let Some(cls) = class_val {
                    if cls.contains("detail-sapo") || cls.contains("article-sapo") {
                        let text = arena.get_text_content(node.id).trim().to_string();
                        if text.chars().count() > 10 {
                            schema_sapo = Some(text);
                        }
                    }
                }
            }

            // Article Body Container
            if schema_body_node.is_none() {
                if matches!(itemprop, Some("articleBody" | "articlebody")) || matches!(data_role, Some("content")) {
                    schema_body_node = Some(node.id);
                }
            }

            // Author
            if schema_author.is_none() {
                if matches!(itemprop, Some("author")) {
                    let text = arena.get_text_content(node.id).trim().to_string();
                    if !text.is_empty() && text.chars().count() < 40 {
                        schema_author = Some(text);
                    }
                } else if let Some(cls) = class_val {
                    if cls.contains("author-info") || cls.contains("author-item-name") || cls.contains("detail-author") {
                        let text = arena.get_text_content(node.id).trim().to_string();
                        if !text.is_empty() && text.chars().count() < 40 {
                            schema_author = Some(text);
                        }
                    }
                }
            }

            // Publication Date
            if schema_date.is_none() {
                if matches!(itemprop, Some("datePublished" | "datepublished")) || matches!(data_role, Some("publishdate")) || tag == "time" {
                    let text = arena.get_text_content(node.id).trim().to_string();
                    if !text.is_empty() && text.chars().count() < 50 {
                        schema_date = Some(normalize_date_display(&text));
                    } else if let Some((_, dt)) = attributes.iter().find(|(k, _)| k == "datetime") {
                        if !dt.is_empty() {
                            schema_date = Some(normalize_date_display(dt));
                        }
                    }
                }
            }
        }
    }

    // Fallback date from URL (htmldate Tier 3)
    if schema_date.is_none() {
        schema_date = extract_date_from_url(base_url);
    }

    // 2. Fall back to class & recursive paragraph / density scoring
    let mut best_node = schema_body_node;
    let mut best_p_count = 0;
    let mut highest_score = -1000i32;

    if best_node.is_none() {
        for node in &arena.nodes {
            if is_noise_or_modal(arena, node.id) {
                continue;
            }
            if let NodeData::Element { tag, attributes } = &node.data {
                let tag_name = tag.as_str();

                // Disqualify noise tags
                if matches!(
                    tag_name,
                    "nav" | "footer" | "header" | "aside" | "script" | "style" | "noscript" | "dialog"
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
                            || lower_val.contains("prose")
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

                // Recursive metrics (text length, punctuation, link density, paragraph count)
                let (total_chars, link_chars, p_count, punct_count) = compute_node_stats(arena, node.id);

                if total_chars > 30 {
                    score += (total_chars / 25) as i32;
                }
                score += (punct_count.min(25) * 2) as i32;

                let link_density = link_chars as f32 / total_chars.max(1) as f32;
                if link_density > 0.5 {
                    score -= 200;
                } else if link_density > 0.25 {
                    score = (score as f32 * (1.0 - link_density * 1.5)) as i32 - 20;
                } else {
                    score += 10;
                }

                if p_count > 0 && score > highest_score {
                    highest_score = score;
                    best_p_count = p_count;
                    best_node = Some(node.id);
                }
            }
        }
    } else if let Some(nid) = schema_body_node {
        let (_, _, p_count, _) = compute_node_stats(arena, nid);
        best_p_count = p_count;
        highest_score = 100;
    }

    let target_node = best_node.unwrap_or(arena.root);

    // Extract title: W3C schema headline first, then target_node subtree, then full document h1, then document <title>, then fallback
    let mut h1_title = schema_headline.unwrap_or_default();
    if h1_title.is_empty() {
        find_first_h1(arena, target_node, &mut h1_title);
    }
    if h1_title.is_empty() {
        for node in &arena.nodes {
            if node.tag() == Some("h1") && !is_noise_or_modal(arena, node.id) {
                let text = arena.get_text_content(node.id).trim().to_string();
                if text.chars().count() > 3 {
                    h1_title = text;
                    break;
                }
            }
        }
    }
    let mut doc_title = String::new();
    if h1_title.is_empty() {
        for node in &arena.nodes {
            if node.tag() == Some("title") {
                let text = arena.get_text_content(node.id).trim().to_string();
                if !text.is_empty() {
                    doc_title = text;
                    break;
                }
            }
        }
    }
    let list_cards = extract_list_cards(arena, base_url, assets);
    let has_h1 = !h1_title.is_empty();
    let is_article = if schema_body_node.is_some() {
        true
    } else if list_cards.len() >= 2 && best_p_count <= 4 {
        false
    } else if is_portal {
        (has_h1 && best_p_count >= 1 && highest_score >= 35) || best_p_count >= 3
    } else {
        best_node.is_some() && (highest_score >= 35 || (has_h1 && best_p_count >= 2) || (best_p_count >= 3 && highest_score >= 20))
    };

    let target_node = if is_article { best_node } else { None };

    let candidate = if !h1_title.is_empty() {
        h1_title
    } else if !doc_title.is_empty() {
        doc_title
    } else {
        String::from(fallback_title)
    };
    let display_title = if is_article { clean_article_title(&candidate, base_url) } else { None };

    // Extract main article content (clean subtree with paragraphs, images, headings)
    let mut article_nodes = Vec::new();
    if let Some(target) = target_node {
        render_clean_subtree(arena, target, &mut article_nodes, assets, base_url);
        if let Some(title) = &display_title {
            article_nodes.retain(|node| !matches!(node, DocNode::Heading { level: 1, text } if text.trim() == title.trim()));
        }
        if let Some(sapo) = &schema_sapo {
            article_nodes.retain(|node| !matches!(node, DocNode::Paragraph { spans } if spans.iter().map(|s| s.text.as_str()).collect::<alloc::string::String>().trim() == sapo.trim()));
        }
    }

    // Assemble clean Markdown-style reader view:
    let mut nodes = Vec::new();

    // 1. Logo (if available) - scaled to compact, elegant reader header
    if let Some(mut logo_node) = find_logo(arena, assets) {
        if let DocNode::DomImage { ref mut width, ref mut height, .. } = logo_node {
            let max_w = 140u32;
            let max_h = 36u32;
            let orig_w = (*width).max(1);
            let orig_h = (*height).max(1);
            let scale_w = (max_w as f32) / (orig_w as f32);
            let scale_h = (max_h as f32) / (orig_h as f32);
            let scale = scale_w.min(scale_h).min(1.0);
            *width = (orig_w as f32 * scale).max(1.0) as u32;
            *height = (orig_h as f32 * scale).max(1.0) as u32;
        }
        nodes.push(logo_node);
    }

    // 2. Main Article Title (H1) - only for actual articles
    if let Some(title) = &display_title {
        nodes.push(DocNode::Heading {
            level: 1,
            text: title.clone(),
        });
    }

    // 2b. Author & Date metadata line
    if is_article {
        let mut meta_parts = Vec::new();
        if let Some(auth) = &schema_author {
            meta_parts.push(auth.as_str());
        }
        if let Some(dt) = &schema_date {
            meta_parts.push(dt.as_str());
        }
        let word_count = count_words_in_nodes(&article_nodes);
        let read_time_label;
        if word_count > 30 {
            let read_mins = (word_count / 180).max(1);
            read_time_label = format!("☕ {} phút đọc", read_mins);
            meta_parts.push(&read_time_label);
        }
        if !meta_parts.is_empty() {
            let meta_text = meta_parts.join(" • ");
            let mut span = StyledSpan::plain(&meta_text);
            span.italic = true;
            span.color = Some(crate::draw::theme::TEXT_MUTED);
            nodes.push(DocNode::Paragraph {
                spans: alloc::vec![span],
            });
        }
    }

    // 2c. Sapo / Lead Paragraph (itemprop="description" or data-role="sapo")
    if is_article {
        if let Some(sapo) = &schema_sapo {
            let mut span = StyledSpan::plain(sapo);
            span.bold = true;
            nodes.push(DocNode::Paragraph {
                spans: alloc::vec![span],
            });
        }
    }

    // 3. Main Article Content (Paragraphs, Headings, Images)
    if !article_nodes.is_empty() {
        nodes.extend(article_nodes);
    }

    // 4. Two Link Groups:
    let (article_links, menu_links) = collect_page_links(arena, base_url);

    // Group 1: Top articles / Read next
    if !nodes.is_empty() && !article_links.is_empty() && is_article {
        nodes.push(DocNode::Rule);
    }

    if is_article {
        if let Some(next_url) = find_next_page_link(arena, base_url) {
            nodes.push(DocNode::Rule);
            let mut span = StyledSpan::plain("📖 Đọc tiếp trang tiếp theo →");
            span.link = Some(next_url);
            span.bold = true;
            span.color = Some(crate::draw::theme::ACCENT_CYAN);
            nodes.push(DocNode::ListItem {
                bullet: '▸',
                indent: 0,
                spans: alloc::vec![span],
            });
        }
        if !article_links.is_empty() {
            for (link_title, link_url) in article_links {
                let mut span = StyledSpan::plain(&link_title);
                span.link = Some(link_url);
                span.bold = true;
                span.color = Some(crate::draw::theme::ACCENT_CYAN);
                nodes.push(DocNode::ListItem {
                    bullet: '•',
                    indent: 0,
                    spans: alloc::vec![span],
                });
            }
        }
    } else {
        // NON-ARTICLE / UNIVERSAL READER VIEW (List / Catalog / Portal / Form)
        // A. Heading for portal / service if not already present
        let portal_title = clean_portal_title(&candidate, base_url);
        if !portal_title.is_empty() && !portal_title.contains("://") {
            nodes.push(DocNode::Heading {
                level: 1,
                text: portal_title.clone(),
            });
        }

        // B. Interactive Native Forms (Search / Login / Feedback)
        let page_forms = extract_page_forms(arena);
        if !page_forms.is_empty() {
            nodes.extend(page_forms);
            nodes.push(DocNode::Rule);
        }

        // C. List / Catalog Cards (News feed, product items, forum threads)
        if !list_cards.is_empty() {
            for card in list_cards {
                if let Some(thumb) = card.thumbnail {
                    nodes.push(thumb);
                }
                let mut span = StyledSpan::plain(&card.title);
                span.link = Some(card.url.clone());
                span.bold = true;
                span.color = Some(crate::draw::theme::ACCENT_CYAN);
                nodes.push(DocNode::ListItem {
                    bullet: '•',
                    indent: 0,
                    spans: alloc::vec![span],
                });
                if let Some(price) = card.price {
                    let mut price_span = StyledSpan::plain(&format!("  💰 {}", price));
                    price_span.bold = true;
                    price_span.color = Some(crate::draw::Color::rgb(166, 227, 161));
                    nodes.push(DocNode::Paragraph { spans: alloc::vec![price_span] });
                }
                if let Some(sapo) = card.sapo {
                    let mut text_span = StyledSpan::plain(&format!("  {}", sapo));
                    text_span.link = Some(card.url.clone());
                    nodes.push(DocNode::Paragraph { spans: alloc::vec![text_span] });
                }
                if let Some(meta) = card.meta {
                    let mut meta_span = StyledSpan::plain(&format!("  {}", meta));
                    meta_span.italic = true;
                    meta_span.color = Some(crate::draw::theme::TEXT_MUTED);
                    nodes.push(DocNode::Paragraph { spans: alloc::vec![meta_span] });
                }
                nodes.push(DocNode::Rule);
            }
        } else if !article_links.is_empty() {
            for (link_title, link_url) in article_links {
                let mut span = StyledSpan::plain(&link_title);
                span.link = Some(link_url);
                span.bold = true;
                span.color = Some(crate::draw::theme::ACCENT_CYAN);
                nodes.push(DocNode::ListItem {
                    bullet: '•',
                    indent: 0,
                    spans: alloc::vec![span],
                });
            }
        }

        // D. Pagination Links
        let pagination = extract_pagination_links(arena, base_url);
        if !pagination.is_empty() {
            nodes.push(DocNode::Rule);
            let mut spans = Vec::new();
            for (i, (pg_text, pg_url)) in pagination.into_iter().enumerate() {
                if i > 0 {
                    spans.push(StyledSpan::plain("  •  "));
                }
                let mut span = StyledSpan::plain(&format!("[ {} ]", pg_text));
                span.link = Some(pg_url);
                span.bold = true;
                span.color = Some(crate::draw::theme::ACCENT_BLUE);
                spans.push(span);
            }
            nodes.push(DocNode::Paragraph { spans });
        }
    }

    // Group 2: Main Menu / Sitemap Navigation
    if !menu_links.is_empty() {
        if !nodes.is_empty() {
            nodes.push(DocNode::Rule);
        }
        for (link_title, link_url) in menu_links {
            let mut span = StyledSpan::plain(&link_title);
            span.link = Some(link_url);
            span.bold = true;
            span.color = Some(crate::draw::theme::ACCENT_BLUE);
            nodes.push(DocNode::ListItem {
                bullet: '▸',
                indent: 0,
                spans: alloc::vec![span],
            });
        }
    }

    let extract_title = display_title.unwrap_or_else(|| {
        clean_portal_title(&candidate, base_url)
    });
    ArticleExtract {
        title: format!("{} [Reader]", extract_title),
        nodes,
    }
}
fn count_words_in_nodes(nodes: &[DocNode]) -> usize {
    let mut words = 0;
    for node in nodes {
        match node {
            DocNode::Heading { text, .. } => words += text.split_whitespace().count(),
            DocNode::Paragraph { spans } => {
                for s in spans {
                    words += s.text.split_whitespace().count();
                }
            }
            DocNode::ListItem { spans, .. } => {
                for s in spans {
                    words += s.text.split_whitespace().count();
                }
            }
            DocNode::Blockquote { lines } | DocNode::CodeBlock { lines, .. } => {
                for line in lines {
                    words += line.split_whitespace().count();
                }
            }
            DocNode::Table { headers, rows } => {
                for h in headers { words += h.split_whitespace().count(); }
                for row in rows {
                    for cell in row { words += cell.split_whitespace().count(); }
                }
            }
            _ => {}
        }
    }
    words
}

fn clean_portal_title(candidate: &str, base_url: &str) -> String {
    let t = candidate.trim();
    if !t.is_empty() && !t.contains("://") {
        return t.to_string();
    }
    base_url.split("://").nth(1).unwrap_or(base_url).split('/').next().unwrap_or(base_url).to_string()
}

fn extract_json_str(json: &str, field: &str) -> Option<String> {
    let key = format!("\"{}\"", field);
    let key_pos = json.find(&key)?;
    let after_key = &json[key_pos + key.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    if after_colon.starts_with('"') {
        let rest = &after_colon[1..];
        let mut out = String::new();
        let mut escape = false;
        for c in rest.chars() {
            if escape {
                out.push(c);
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                break;
            } else {
                out.push(c);
            }
        }
        let trimmed = out.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn extract_json_author(json: &str) -> Option<String> {
    if let Some(author_pos) = json.find("\"author\"") {
        let after = &json[author_pos + 8..];
        if let Some(colon) = after.find(':') {
            let val = after[colon + 1..].trim_start();
            if val.starts_with('"') {
                return extract_json_str(json, "author");
            } else if val.starts_with('{') || val.starts_with('[') {
                let block_end = val.find('}').unwrap_or(val.len());
                let author_block = &val[..block_end];
                return extract_json_str(author_block, "name");
            }
        }
    }
    None
}

fn extract_json_ld_metadata(arena: &DocumentArena) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    let mut headline = None;
    let mut sapo = None;
    let mut author = None;
    let mut date = None;

    for node in &arena.nodes {
        if node.tag() == Some("script") {
            if let Some(kind) = node.get_attribute("type") {
                if kind.trim().eq_ignore_ascii_case("application/ld+json") {
                    let content = arena.get_text_content(node.id);
                    if content.is_empty() {
                        continue;
                    }
                    if headline.is_none() {
                        headline = extract_json_str(&content, "headline")
                            .or_else(|| extract_json_str(&content, "name"));
                    }
                    if sapo.is_none() {
                        sapo = extract_json_str(&content, "description");
                    }
                    if date.is_none() {
                        if let Some(d) = extract_json_str(&content, "datePublished")
                            .or_else(|| extract_json_str(&content, "dateCreated"))
                            .or_else(|| extract_json_str(&content, "dateModified"))
                        {
                            date = Some(normalize_date_display(&d));
                        }
                    }
                    if author.is_none() {
                        author = extract_json_author(&content);
                    }
                }
            }
        }
    }
    (headline, sapo, author, date)
}

fn extract_meta_tags(arena: &DocumentArena) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    let mut title = None;
    let mut sapo = None;
    let mut author = None;
    let mut date = None;

    for node in &arena.nodes {
        if node.tag() == Some("meta") {
            let mut prop_or_name: Option<&str> = None;
            let mut content: Option<&str> = None;
            if let NodeData::Element { attributes, .. } = &node.data {
                for (k, v) in attributes {
                    if k == "property" || k == "name" {
                        prop_or_name = Some(v.as_str());
                    } else if k == "content" {
                        content = Some(v.as_str());
                    }
                }
            }
            if let (Some(pn), Some(cnt)) = (prop_or_name, content) {
                let cnt = cnt.trim();
                if cnt.is_empty() {
                    continue;
                }
                let lower_pn = pn.to_ascii_lowercase();
                if date.is_none() {
                    if matches!(
                        lower_pn.as_str(),
                        "article:published_time"
                            | "og:article:published_time"
                            | "pubdate"
                            | "publishdate"
                            | "date"
                            | "dc.date"
                            | "dc.date.issued"
                            | "dc:date"
                            | "parsely-pub-date"
                            | "sailthru.date"
                            | "rnews:datepublished"
                    ) {
                        date = Some(normalize_date_display(cnt));
                    }
                }
                if title.is_none() {
                    if matches!(lower_pn.as_str(), "og:title" | "twitter:title") && cnt.chars().count() > 3 {
                        title = Some(cnt.to_string());
                    }
                }
                if sapo.is_none() {
                    if matches!(lower_pn.as_str(), "og:description" | "description" | "twitter:description") && cnt.chars().count() > 10 {
                        sapo = Some(cnt.to_string());
                    }
                }
                if author.is_none() {
                    if matches!(lower_pn.as_str(), "author" | "article:author" | "byl" | "dc.creator") && cnt.chars().count() < 50 {
                        author = Some(cnt.to_string());
                    }
                }
            }
        }
    }
    (title, sapo, author, date)
}

fn normalize_date_display(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 10 {
        let bytes = trimmed.as_bytes();
        if bytes[0..4].iter().all(|b| b.is_ascii_digit())
            && bytes[4] == b'-'
            && bytes[5..7].iter().all(|b| b.is_ascii_digit())
            && bytes[7] == b'-'
            && bytes[8..10].iter().all(|b| b.is_ascii_digit())
        {
            let year = &trimmed[0..4];
            let month = &trimmed[5..7];
            let day = &trimmed[8..10];
            return format!("{}/{}/{}", day, month, year);
        }
        if bytes[0..4].iter().all(|b| b.is_ascii_digit())
            && bytes[4] == b'/'
            && bytes[5..7].iter().all(|b| b.is_ascii_digit())
            && bytes[7] == b'/'
            && bytes[8..10].iter().all(|b| b.is_ascii_digit())
        {
            let year = &trimmed[0..4];
            let month = &trimmed[5..7];
            let day = &trimmed[8..10];
            return format!("{}/{}/{}", day, month, year);
        }
    }
    trimmed.to_string()
}

fn extract_date_from_url(url: &str) -> Option<String> {
    let bytes = url.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    for i in 0..bytes.len() - 9 {
        let sep1 = bytes[i];
        if (sep1 == b'/' || sep1 == b'-') && i + 10 < bytes.len() {
            let y_slice = &bytes[i + 1..i + 5];
            let sep2 = bytes[i + 5];
            let m_slice = &bytes[i + 6..i + 8];
            let sep3 = bytes[i + 8];
            let d_slice = &bytes[i + 9..i + 11];
            if (sep2 == b'/' || sep2 == b'-')
                && (sep3 == b'/' || sep3 == b'-')
                && y_slice.iter().all(|b| b.is_ascii_digit())
                && m_slice.iter().all(|b| b.is_ascii_digit())
                && d_slice.iter().all(|b| b.is_ascii_digit())
            {
                let year: u32 = core::str::from_utf8(y_slice).ok()?.parse().ok()?;
                let month: u32 = core::str::from_utf8(m_slice).ok()?.parse().ok()?;
                let day: u32 = core::str::from_utf8(d_slice).ok()?.parse().ok()?;
                if (1995..=2035).contains(&year) && (1..=12).contains(&month) && (1..=31).contains(&day) {
                    return Some(format!("{:02}/{:02}/{}", day, month, year));
                }
            }
        }
    }
    None
}

fn compute_node_stats(arena: &DocumentArena, node_id: NodeId) -> (usize, usize, usize, usize) {
    let mut total_chars = 0;
    let mut link_chars = 0;
    let mut p_count = 0;
    let mut punct_count = 0;

    fn walk(
        arena: &DocumentArena,
        id: NodeId,
        in_link: bool,
        total: &mut usize,
        link: &mut usize,
        p_cnt: &mut usize,
        punct: &mut usize,
    ) {
        let Some(node) = arena.get(id) else { return; };
        if is_noise_or_modal(arena, id) { return; }
        match &node.data {
            NodeData::Element { tag, .. } => {
                let is_link = in_link || tag == "a";
                if tag == "p" {
                    *p_cnt += 1;
                }
                let mut child = node.first_child;
                while let Some(cid) = child {
                    walk(arena, cid, is_link, total, link, p_cnt, punct);
                    child = arena.get(cid).and_then(|n| n.next_sibling);
                }
            }
            NodeData::Text(txt) => {
                let count = txt.chars().count();
                *total += count;
                if in_link {
                    *link += count;
                }
                for c in txt.chars() {
                    if matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | '—' | '-') {
                        *punct += 1;
                    }
                }
            }
            _ => {}
        }
    }

    walk(arena, node_id, false, &mut total_chars, &mut link_chars, &mut p_count, &mut punct_count);
    (total_chars, link_chars, p_count, punct_count)
}

fn collect_single_input(arena: &DocumentArena, id: NodeId) -> Option<DocNode> {
    let node = arena.get(id)?;
    if let NodeData::Element { tag, attributes } = &node.data {
        if tag == "input" {
            let mut input_type = String::from("text");
            let mut name = String::new();
            let mut value = String::new();
            let mut placeholder = String::new();
            let mut checked = false;
            for (k, v) in attributes {
                match k.as_str() {
                    "type" => input_type = v.to_ascii_lowercase(),
                    "name" => name = v.clone(),
                    "value" => value = v.clone(),
                    "placeholder" => placeholder = v.clone(),
                    "checked" => checked = true,
                    _ => {}
                }
            }
            if input_type != "hidden" {
                return Some(DocNode::Input {
                    input_type,
                    name,
                    value,
                    placeholder,
                    checked,
                    node_id: Some(id),
                });
            }
        }
    }
    None
}

fn collect_single_button(arena: &DocumentArena, id: NodeId) -> Option<DocNode> {
    let node = arena.get(id)?;
    if let NodeData::Element { tag, attributes } = &node.data {
        if tag == "button" {
            let btn_type = attributes
                .iter()
                .find(|(k, _)| k == "type")
                .map(|(_, v)| v.as_str())
                .unwrap_or("submit");
            let value = arena.get_text_content(id).trim().to_string();
            let label = if value.is_empty() { String::from("Submit") } else { value };
            return Some(DocNode::Input {
                input_type: String::from(btn_type),
                name: attributes.iter().find(|(k, _)| k == "name").map(|(_, v)| v.clone()).unwrap_or_default(),
                value: label,
                placeholder: String::new(),
                checked: false,
                node_id: Some(id),
            });
        }
    }
    None
}

fn collect_single_textarea(arena: &DocumentArena, id: NodeId) -> Option<DocNode> {
    let node = arena.get(id)?;
    if let NodeData::Element { tag, attributes } = &node.data {
        if tag == "textarea" {
            let text = arena.get_text_content(id).trim().to_string();
            let name = attributes.iter().find(|(k, _)| k == "name").map(|(_, v)| v.clone()).unwrap_or_default();
            let placeholder = attributes.iter().find(|(k, _)| k == "placeholder").map(|(_, v)| v.clone()).unwrap_or_default();
            return Some(DocNode::Input {
                input_type: String::from("textarea"),
                name,
                value: text,
                placeholder,
                checked: false,
                node_id: Some(id),
            });
        }
    }
    None
}

fn collect_form_nodes(arena: &DocumentArena, form_id: NodeId, out: &mut Vec<DocNode>) {
    fn walk_form(arena: &DocumentArena, nid: NodeId, out: &mut Vec<DocNode>) {
        let Some(node) = arena.get(nid) else { return; };
        if is_noise_or_modal(arena, nid) { return; }
        if let NodeData::Element { tag, .. } = &node.data {
            match tag.as_str() {
                "input" => {
                    if let Some(inp) = collect_single_input(arena, nid) {
                        out.push(inp);
                    }
                }
                "button" => {
                    if let Some(btn) = collect_single_button(arena, nid) {
                        out.push(btn);
                    }
                }
                "textarea" => {
                    if let Some(ta) = collect_single_textarea(arena, nid) {
                        out.push(ta);
                    }
                }
                "label" => {
                    let text = arena.get_text_content(nid).trim().to_string();
                    if !text.is_empty() {
                        let mut span = StyledSpan::plain(&text);
                        span.bold = true;
                        out.push(DocNode::Paragraph { spans: alloc::vec![span] });
                    }
                }
                _ => {
                    let mut child = node.first_child;
                    while let Some(cid) = child {
                        walk_form(arena, cid, out);
                        child = arena.get(cid).and_then(|n| n.next_sibling);
                    }
                }
            }
        }
    }

    let Some(form_node) = arena.get(form_id) else { return; };
    let mut child = form_node.first_child;
    while let Some(cid) = child {
        walk_form(arena, cid, out);
        child = arena.get(cid).and_then(|n| n.next_sibling);
    }
}

fn extract_page_forms(arena: &DocumentArena) -> Vec<DocNode> {
    let mut out = Vec::new();
    for node in &arena.nodes {
        if node.tag() == Some("form") && !is_noise_or_modal(arena, node.id) {
            collect_form_nodes(arena, node.id, &mut out);
        }
    }
    out
}

pub struct ListCard {
    pub title: String,
    pub url: String,
    pub sapo: Option<String>,
    pub price: Option<String>,
    pub meta: Option<String>,
    pub thumbnail: Option<DocNode>,
}

fn extract_single_card(
    arena: &DocumentArena,
    card_id: NodeId,
    base_url: &str,
    seen_urls: &mut BTreeSet<String>,
    assets: &[DocNode],
) -> Option<ListCard> {
    let mut best_link: Option<(String, String)> = None;
    let mut longest_link_text = 0;

    let mut sapo: Option<String> = None;
    let mut price: Option<String> = None;
    let mut author: Option<String> = None;
    let mut date: Option<String> = None;
    let mut replies: Option<String> = None;
    let mut thumbnail: Option<DocNode> = None;
    let mut stack = alloc::vec![card_id];
    while let Some(nid) = stack.pop() {
        let Some(node) = arena.get(nid) else { continue; };
        if is_noise_or_modal(arena, nid) { continue; }

        if let NodeData::Element { tag, attributes } = &node.data {
            let tag_name = tag.as_str();
            let mut class_val = "";
            let mut href_val = "";
            let mut dt_val = "";
            for (k, v) in attributes {
                if k == "class" { class_val = v.as_str(); }
                else if k == "href" { href_val = v.as_str(); }
                else if k == "datetime" { dt_val = v.as_str(); }
            }
            let lower_class = class_val.to_ascii_lowercase();
            if thumbnail.is_none() && tag_name == "img" {
                if let Some(image) = assets.iter().find(|asset| matches!(asset, DocNode::DomImage { node_id, .. } if *node_id == nid)) {
                    if let DocNode::DomImage { width, height, pixels, .. } = image {
                        let max_w = 200u32;
                        let max_h = 120u32;
                        let orig_w = (*width).max(1);
                        let orig_h = (*height).max(1);
                        let scale_w = (max_w as f32) / (orig_w as f32);
                        let scale_h = (max_h as f32) / (orig_h as f32);
                        let scale = scale_w.min(scale_h).min(1.0);
                        let thumb_w = (orig_w as f32 * scale).max(1.0) as u32;
                        let thumb_h = (orig_h as f32 * scale).max(1.0) as u32;
                        thumbnail = Some(DocNode::LinkedImage {
                            width: thumb_w,
                            height: thumb_h,
                            pixels: pixels.clone(),
                            link: String::new(),
                        });
                    }
                }
            }
            if tag_name == "a" && !href_val.is_empty() {
                let href = href_val.trim();
                if let Some(resolved) = crate::resources::resolve_hyperlink(base_url, href) {
                    let link_text = arena.get_text_content(nid).trim().to_string();
                    if link_text.chars().count() >= 5 {
                        let is_heading_anchor = lower_class.contains("title")
                             || lower_class.contains("heading")
                             || lower_class.contains("name")
                             || node.parent.and_then(|p| arena.get(p)).and_then(|pn| pn.tag()).map_or(false, |t| t.starts_with('h'));

                        if is_heading_anchor || link_text.chars().count() > longest_link_text {
                            longest_link_text = link_text.chars().count();
                            best_link = Some((resolved, link_text));
                        }
                    }
                }
            }

            if price.is_none() {
                if lower_class.contains("price") || lower_class.contains("cost") {
                    let txt = arena.get_text_content(nid).trim().to_string();
                    if !txt.is_empty() && txt.chars().count() < 40 {
                        price = Some(txt);
                    }
                } else {
                    let txt = arena.get_text_content(nid).trim().to_string();
                    if (txt.contains('đ') || txt.contains("VND") || txt.contains('$') || txt.contains('€'))
                        && txt.chars().any(|c| c.is_ascii_digit()) && txt.chars().count() < 30
                    {
                        price = Some(txt);
                    }
                }
            }

            if sapo.is_none() && (tag_name == "p" || lower_class.contains("sapo") || lower_class.contains("desc") || lower_class.contains("summary")) {
                let txt = arena.get_text_content(nid).trim().to_string();
                if txt.chars().count() >= 10 && txt.chars().count() <= 350 {
                    sapo = Some(txt);
                }
            }

            if date.is_none() {
                if tag_name == "time" || lower_class.contains("date") || lower_class.contains("time") {
                    let txt = if !dt_val.is_empty() { normalize_date_display(dt_val) } else { arena.get_text_content(nid).trim().to_string() };
                    if !txt.is_empty() && txt.chars().count() < 40 {
                        date = Some(txt);
                    }
                }
            }

            if author.is_none() && (lower_class.contains("author") || lower_class.contains("user") || lower_class.contains("byline")) {
                let txt = arena.get_text_content(nid).trim().to_string();
                if !txt.is_empty() && txt.chars().count() < 40 {
                    author = Some(txt);
                }
            }

            if replies.is_none() && (lower_class.contains("reply") || lower_class.contains("comment") || lower_class.contains("response")) {
                let txt = arena.get_text_content(nid).trim().to_string();
                if !txt.is_empty() && txt.chars().count() < 30 && txt.chars().any(|c| c.is_ascii_digit()) {
                    replies = Some(txt);
                }
            }

            let mut child = node.first_child;
            while let Some(cid) = child {
                stack.push(cid);
                child = arena.get(cid).and_then(|n| n.next_sibling);
            }
        }
    }

    let (url, title) = best_link?;
    if seen_urls.contains(&url) {
        return None;
    }
    seen_urls.insert(url.clone());

    if let Some(s) = &sapo {
        if s.trim() == title.trim() {
            sapo = None;
        }
    }

    let mut meta_parts = Vec::new();
    if let Some(a) = author {
        meta_parts.push(a);
    }
    if let Some(d) = date {
        meta_parts.push(d);
    }
    if let Some(r) = replies {
        meta_parts.push(format!("💬 {}", r));
    }
    let meta = if !meta_parts.is_empty() {
        Some(meta_parts.join(" • "))
    } else {
        None
    };

    if let Some(DocNode::LinkedImage { ref mut link, .. }) = thumbnail {
        *link = url.clone();
    }

    Some(ListCard {
        title,
        url,
        sapo,
        price,
        meta,
        thumbnail,
    })
}

fn is_card_container_class(lower: &str) -> bool {
    lower.contains("list")
        || lower.contains("grid")
        || lower.contains("feed")
        || lower.contains("catalog")
        || lower.contains("container")
        || lower.contains("wrapper")
}

fn is_card_item_class(lower: &str) -> bool {
    (lower.contains("card")
        || lower.contains("item")
        || lower.contains("post")
        || lower.contains("thread")
        || lower.contains("product")
        || lower.contains("entry")
        || lower.contains("row")
        || lower.contains("news"))
        && !is_card_container_class(lower)
}

fn extract_list_cards(arena: &DocumentArena, base_url: &str, assets: &[DocNode]) -> Vec<ListCard> {
    let mut cards = Vec::new();
    let mut seen_urls = BTreeSet::new();

    let article_nodes: Vec<NodeId> = arena.nodes.iter()
        .filter(|n| n.tag() == Some("article") && !is_noise_or_modal(arena, n.id))
        .map(|n| n.id)
        .collect();

    let candidate_container_nodes = if article_nodes.len() >= 2 {
        article_nodes
    } else {
        let mut list = Vec::new();
        for node in &arena.nodes {
            if is_noise_or_modal(arena, node.id) { continue; }
            if let NodeData::Element { tag, attributes } = &node.data {
                let tag_name = tag.as_str();
                if matches!(tag_name, "div" | "section" | "li" | "tr") {
                    let mut is_item = false;
                    let mut is_container = false;
                    for (k, v) in attributes {
                        if k == "class" || k == "id" {
                            let lower = v.to_ascii_lowercase();
                            if is_card_item_class(&lower) {
                                is_item = true;
                                break;
                            } else if is_card_container_class(&lower) {
                                is_container = true;
                            }
                        }
                    }
                    if is_item {
                        list.push(node.id);
                    } else if is_container && tag_name != "tr" {
                        let mut child = node.first_child;
                        while let Some(cid) = child {
                            if let Some(cnode) = arena.get(cid) {
                                if let NodeData::Element { attributes: cattrs, .. } = &cnode.data {
                                    if !is_noise_or_modal(arena, cid) {
                                        let child_has_item_class = cattrs.iter().any(|(ck, cv)| {
                                            (ck == "class" || ck == "id") && is_card_item_class(&cv.to_ascii_lowercase())
                                        });
                                        if !child_has_item_class {
                                            list.push(cid);
                                        }
                                    }
                                }
                            }
                            child = arena.get(cid).and_then(|n| n.next_sibling);
                        }
                    }
                }
            }
        }
        list
    };

    for card_id in candidate_container_nodes {
        if let Some(card) = extract_single_card(arena, card_id, base_url, &mut seen_urls, assets) {
            cards.push(card);
        }
    }

    cards
}

fn extract_pagination_links(arena: &DocumentArena, base_url: &str) -> Vec<(String, String)> {
    let mut pages = Vec::new();
    let mut seen = BTreeSet::new();

    for node in &arena.nodes {
        if is_noise_or_modal(arena, node.id) { continue; }
        if let NodeData::Element { tag, attributes } = &node.data {
            if tag == "a" {
                let mut href_val = "";
                let mut class_val = "";
                for (k, v) in attributes {
                    if k == "href" { href_val = v.as_str(); }
                    else if k == "class" { class_val = v.as_str(); }
                }
                let lower_class = class_val.to_ascii_lowercase();
                let text = arena.get_text_content(node.id).trim().to_string();
                let lower_text = text.to_ascii_lowercase();

                let is_pagination = lower_class.contains("pagination")
                    || lower_class.contains("page-link")
                    || lower_class.contains("pager")
                    || matches!(
                        lower_text.as_str(),
                        "trang sau" | "trang trước" | "tiếp" | "lùi" | "next" | "prev" | "previous" | "»" | "«" | ">" | "<"
                    );

                if is_pagination && !href_val.is_empty() {
                    if let Some(resolved) = crate::resources::resolve_hyperlink(base_url, href_val) {
                        if !seen.contains(&resolved) {
                            seen.insert(resolved.clone());
                            pages.push((text, resolved));
                        }
                    }
                }
            }
        }
    }
    pages
}

fn is_portal_or_homepage(base_url: &str) -> bool {
    let without_scheme = base_url.split("://").nth(1).unwrap_or(base_url);
    let path = without_scheme.split('/').nth(1).unwrap_or("");
    path.is_empty() || path == "index.html" || path == "index.htm" || path == "index.php"
}

fn clean_article_title(title: &str, base_url: &str) -> Option<String> {
    let t = title.trim();
    if t.is_empty() || t.contains("://") {
        return None;
    }
    for prefix in ["Ocel — ", "Ocel - "] {
        if let Some(rest) = t.strip_prefix(prefix) {
            let rest = rest.trim();
            if rest.contains("://") || rest.eq_ignore_ascii_case(base_url) {
                return None;
            }
            if !rest.is_empty() {
                return Some(String::from(rest));
            }
        }
    }
    let host = base_url.split("://").nth(1).unwrap_or(base_url).trim_end_matches('/');
    if t.eq_ignore_ascii_case(base_url) || t.trim_end_matches('/') == host {
        return None;
    }
    Some(String::from(t))
}


fn find_first_h1(arena: &DocumentArena, id: NodeId, out: &mut String) {
    let Some(node) = arena.get(id) else { return; };
    if node.tag() == Some("h1") {
        let text = arena.get_text_content(id).trim().to_string();
        if text.chars().count() > 3 {
            *out = text;
            return;
        }
    }
    let mut child = node.first_child;
    while let Some(cid) = child {
        find_first_h1(arena, cid, out);
        if !out.is_empty() {
            return;
        }
        child = arena.get(cid).and_then(|n| n.next_sibling);
    }
}

fn find_next_page_link(arena: &DocumentArena, base_url: &str) -> Option<String> {
    for node in &arena.nodes {
        if is_noise_or_modal(arena, node.id) { continue; }
        if let NodeData::Element { tag, attributes } = &node.data {
            let tag_name = tag.as_str();
            if tag_name == "link" || tag_name == "a" {
                let mut rel_val = "";
                let mut href_val = "";
                let mut class_val = "";
                for (k, v) in attributes {
                    if k == "rel" { rel_val = v.as_str(); }
                    else if k == "href" { href_val = v.as_str(); }
                    else if k == "class" { class_val = v.as_str(); }
                }
                let is_next = rel_val.eq_ignore_ascii_case("next")
                    || class_val.to_ascii_lowercase().contains("next-page")
                    || class_val.to_ascii_lowercase().contains("page-next");
                if is_next && !href_val.is_empty() {
                    if let Some(resolved) = crate::resources::resolve_hyperlink(base_url, href_val) {
                        return Some(resolved);
                    }
                }
            }
        }
    }
    None
}

fn find_logo(arena: &DocumentArena, assets: &[DocNode]) -> Option<DocNode> {
    for node in &arena.nodes {
        if let NodeData::Element { tag, attributes } = &node.data {
            if tag == "img" {
                let is_logo = attributes.iter().any(|(k, v)| {
                    let lower = v.to_ascii_lowercase();
                    (k == "class" || k == "id" || k == "alt" || k == "src")
                        && (lower.contains("logo") || lower.contains("brand"))
                });
                if is_logo {
                    if let Some(image) = assets.iter().find(|asset| matches!(asset, DocNode::DomImage { node_id, .. } if *node_id == node.id)) {
                        return Some(image.clone());
                    }
                }
            }
        }
    }
    for node in &arena.nodes {
        if node.tag() == Some("header") {
            let mut stack = alloc::vec![node.id];
            while let Some(current) = stack.pop() {
                if let Some(image) = assets.iter().find(|asset| matches!(asset, DocNode::DomImage { node_id, .. } if *node_id == current)) {
                    return Some(image.clone());
                }
                if let Some(n) = arena.get(current) {
                    let mut child = n.first_child;
                    while let Some(c) = child {
                        stack.push(c);
                        child = arena.get(c).and_then(|x| x.next_sibling);
                    }
                }
            }
        }
    }
    None
}

fn is_in_nav_or_header(arena: &DocumentArena, id: NodeId) -> bool {
    let mut cur = Some(id);
    while let Some(nid) = cur {
        if let Some(node) = arena.get(nid) {
            if let NodeData::Element { tag, attributes } = &node.data {
                let tag_name = tag.as_str();
                if matches!(tag_name, "nav" | "footer" | "header" | "aside") {
                    return true;
                }
                for (k, v) in attributes {
                    if k == "class" || k == "id" {
                        let lower = v.to_ascii_lowercase();
                        if lower.contains("menu")
                            || lower.contains("nav")
                            || lower.contains("header")
                            || lower.contains("footer")
                            || lower.contains("sidebar")
                            || lower.contains("topbar")
                            || lower.contains("top-bar")
                        {
                            return true;
                        }
                    }
                }
            }
            cur = node.parent;
        } else {
            break;
        }
    }
    false
}

fn is_noise_or_modal(arena: &DocumentArena, id: NodeId) -> bool {
    let mut cur = Some(id);
    while let Some(nid) = cur {
        if let Some(node) = arena.get(nid) {
            if let NodeData::Element { tag, attributes } = &node.data {
                let tag_name = tag.as_str();
                if matches!(tag_name, "script" | "style" | "noscript" | "dialog") {
                    return true;
                }
                for (k, v) in attributes {
                    if k == "class" || k == "id" {
                        let lower = v.to_ascii_lowercase();
                        if lower.contains("modal")
                            || lower.contains("popup")
                            || lower.contains("dialog")
                            || lower.contains("overlay")
                            || lower.contains("banner")
                            || lower.contains("cookie")
                            || lower.contains("lightbox")
                            || lower.contains("advertisement")
                            || lower.contains("subscribe")
                        {
                            return true;
                        }
                    }
                }
            }
            cur = node.parent;
        } else {
            break;
        }
    }
    false
}

fn collect_page_links(arena: &DocumentArena, base_url: &str) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let mut article_map: alloc::collections::BTreeMap<String, String> = alloc::collections::BTreeMap::new();
    let mut general_map: alloc::collections::BTreeMap<String, String> = alloc::collections::BTreeMap::new();

    for node in &arena.nodes {
        if let NodeData::Element { tag, attributes } = &node.data {
            if tag == "a" {
                if is_noise_or_modal(arena, node.id) {
                    continue;
                }
                if let Some((_, href)) = attributes.iter().find(|(k, _)| k == "href") {
                    let href = href.trim();
                    if href.is_empty()
                        || href.starts_with('#')
                        || href.starts_with("javascript:")
                        || href.starts_with("mailto:")
                        || href.starts_with("tel:")
                    {
                        continue;
                    }
                    let resolved = match crate::resources::resolve_hyperlink(base_url, href) {
                        Some(url) => url,
                        None => continue,
                    };
                    if resolved.trim_end_matches('/') == base_url.trim_end_matches('/') {
                        continue;
                    }
                    let mut text = arena.get_text_content(node.id).trim().to_string();
                    if text.is_empty() {
                        if let Some((_, title)) = attributes.iter().find(|(k, _)| k == "title" || k == "aria-label") {
                            text = title.trim().to_string();
                        }
                    }
                    let words: Vec<&str> = text.split_whitespace().collect();
                    let clean_text = words.join(" ");
                    if clean_text.chars().count() < 2 || clean_text == "..." {
                        continue;
                    }
                    let lower = clean_text.to_ascii_lowercase();
                    if matches!(
                        lower.as_str(),
                        "xem thêm"
                            | "chi tiết"
                            | "đọc tiếp"
                            | "tại đây"
                            | "quên mật khẩu?"
                            | "đăng nhập"
                            | "đăng ký"
                            | "bình luận"
                            | "quảng cáo"
                    ) {
                        continue;
                    }
                    if href.contains("/tac-gia/") {
                        continue;
                    }

                    let in_nav = is_in_nav_or_header(arena, node.id);
                    let slug = href.split('/').next_back().unwrap_or(href).split('?').next().unwrap_or(href);
                    let is_article_slug = slug.ends_with(".htm") || slug.ends_with(".html");
                    let has_id_or_dashes = (slug.chars().any(|c| c.is_ascii_digit()) && slug.contains('-'))
                        || slug.matches('-').count() >= 3;
                    let is_headline = clean_text.chars().count() >= 18;

                    if (!in_nav && (has_id_or_dashes || is_headline))
                        || (is_article_slug && has_id_or_dashes && !in_nav)
                    {
                        let existing = article_map.entry(resolved).or_default();
                        if clean_text.len() > existing.len() {
                            *existing = clean_text;
                        }
                    } else if (in_nav || (is_article_slug && slug.matches('-').count() <= 1))
                        && clean_text.chars().count() <= 25
                    {
                        if !resolved.contains("google.com")
                            && !resolved.contains("facebook.com")
                            && !resolved.contains("youtube.com")
                        {
                            let existing = general_map.entry(resolved).or_default();
                            if clean_text.len() > existing.len() {
                                *existing = clean_text;
                            }
                        }
                    }
                }
            }
        }
    }

    let mut articles = Vec::new();
    for (url, title) in article_map {
        articles.push((title, url));
    }
    let mut menus = Vec::new();
    for (url, title) in general_map {
        if menus.len() >= 25 {
            break;
        }
        menus.push((title, url));
    }
    (articles, menus)
}

fn render_clean_subtree(
    arena: &DocumentArena,
    parent_id: NodeId,
    out: &mut Vec<DocNode>,
    assets: &[DocNode],
    base_url: &str,
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
                    if is_noise_or_modal(arena, child_id)
                        || matches!(
                            tag_name,
                            "script" | "style" | "nav" | "footer" | "header" | "aside" | "noscript"
                        )
                    {
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
                            let mut spans = Vec::new();
                            collect_article_spans(arena, child_id, &mut spans, base_url, false, false, false, None);
                            if !spans.is_empty() {
                                out.push(DocNode::Paragraph { spans });
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
                        "figure" => {
                            render_clean_subtree(arena, child_id, out, assets, base_url);
                        }
                        "figcaption" => {
                            let text = String::from(arena.get_text_content(child_id).trim());
                            if !text.is_empty() {
                                let mut span = StyledSpan::plain(&text);
                                span.italic = true;
                                span.color = Some(crate::draw::theme::TEXT_MUTED);
                                out.push(DocNode::Paragraph { spans: alloc::vec![span] });
                            }
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
                        "form" => {
                            collect_form_nodes(arena, child_id, out);
                        }
                        "input" => {
                            if let Some(inp) = collect_single_input(arena, child_id) {
                                out.push(inp);
                            }
                        }
                        "button" => {
                            if let Some(btn) = collect_single_button(arena, child_id) {
                                out.push(btn);
                            }
                        }
                        "textarea" => {
                            if let Some(ta) = collect_single_textarea(arena, child_id) {
                                out.push(ta);
                            }
                        }
                        _ => {
                            // Recursively extract from div, section, etc.
                            render_clean_subtree(arena, child_id, out, assets, base_url);
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
fn collect_article_spans(
    arena: &DocumentArena,
    node_id: NodeId,
    out: &mut Vec<StyledSpan>,
    base_url: &str,
    bold: bool,
    italic: bool,
    code: bool,
    link: Option<String>,
) {
    let Some(node) = arena.get(node_id) else { return; };
    let mut cur = node.first_child;
    while let Some(child_id) = cur {
        if let Some(child) = arena.get(child_id) {
            match &child.data {
                NodeData::Element { tag, attributes } => {
                    if matches!(tag.as_str(), "script" | "style" | "noscript") {
                        cur = child.next_sibling;
                        continue;
                    }
                    let next_bold = bold || matches!(tag.as_str(), "b" | "strong" | "h1" | "h2" | "h3");
                    let next_italic = italic || matches!(tag.as_str(), "i" | "em");
                    let next_code = code || tag == "code";
                    let next_link = if tag == "a" {
                        attributes
                            .iter()
                            .find(|(k, _)| k == "href")
                            .and_then(|(_, href)| {
                                crate::resources::resolve_hyperlink(base_url, href)
                            })
                            .or(link.clone())
                    } else {
                        link.clone()
                    };

                    collect_article_spans(
                        arena, child_id, out, base_url, next_bold, next_italic, next_code, next_link,
                    );
                }
                NodeData::Text(text) if !text.is_empty() => {
                    let color = if link.is_some() {
                        Some(crate::draw::theme::ACCENT_CYAN)
                    } else if code {
                        Some(crate::draw::theme::ACCENT_BLUE)
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

    #[test]
    fn test_reader_extracts_article_images_logo_and_links() {
        let html = r#"
            <html>
            <body>
                <header>
                    <img class="site-logo" src="/brand/logo.png" alt="News Portal">
                    <nav>
                        <a href="/category/tech">Technology News</a>
                        <a href="/category/world">World News</a>
                        <a href="javascript:void(0)">Ignored</a>
                    </nav>
                </header>
                <article>
                    <h1>Breaking Scientific Discovery</h1>
                    <figure>
                        <img src="/photos/mars.jpg" alt="Mars Surface">
                        <figcaption>High-resolution panorama of Mars</figcaption>
                    </figure>
                    <p>Scientists have uncovered compelling evidence of subterranean liquid reservoirs on Mars.</p>
                </article>
            </body>
            </html>
        "#;
        let parsed = crate::parser::html::parse_html(html);
        let extracted = extract_article(&parsed.arena, "https://news.test/mars-discovery", "News");
        assert_eq!(extracted.title, "Breaking Scientific Discovery [Reader]");
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::Heading { level: 1, text } if text == "Breaking Scientific Discovery")));
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::Paragraph { spans } if spans.iter().any(|s| s.text.contains("High-resolution panorama")))));
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::Paragraph { spans } if spans.iter().any(|s| s.text.contains("Scientists have uncovered")))));
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::Rule)));
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::ListItem { spans, .. } if spans.iter().any(|s| s.text == "Technology News" && s.link.as_deref() == Some("https://news.test/category/tech")))));
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::ListItem { spans, .. } if spans.iter().any(|s| s.text == "World News" && s.link.as_deref() == Some("https://news.test/category/world")))));
        assert!(!extracted.nodes.iter().any(|node| matches!(node, DocNode::ListItem { spans, .. } if spans.iter().any(|s| s.text == "Ignored"))));
    }
    #[test]
    fn test_reader_w3c_schema_extracts_full_article_and_navigation_groups() {
        let html = r#"
            <html>
            <body>
                <header class="header">
                    <nav class="main-menu">
                        <a href="/thoi-su.htm">Thời sự</a>
                        <a href="/kinh-doanh.htm">Kinh doanh</a>
                    </nav>
                </header>
                <div class="detail-container">
                    <h1 itemprop="headline">Đảng viên làm giàu chính đáng</h1>
                    <div class="author-info" itemprop="author">HÀ SỸ ĐỒNG</div>
                    <time itemprop="datePublished" datetime="2026-10-18T10:00:00">18/10/2026</time>
                    <p itemprop="description">Tạo điều kiện khuyến khích làm giàu chính đáng phát triển đất nước.</p>
                    <div itemprop="articleBody">
                        <p>Đoạn văn thân bài thứ nhất với đầy đủ nội dung phân tích chi tiết.</p>
                        <p>Đoạn văn thân bài thứ hai bổ sung luận điểm sâu sắc và giải pháp.</p>
                    </div>
                </div>
                <div class="related-news">
                    <a href="/tin-tuc-thoi-su-nong-1002610.htm">Tin tức thời sự quan trọng tiếp theo</a>
                </div>
            </body>
            </html>
        "#;
        let parsed = crate::parser::html::parse_html(html);
        let extracted = extract_article(&parsed.arena, "https://tuoitre.vn/bai-viet.htm", "Tuổi Trẻ");
        // Title
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::Heading { level: 1, text } if text.contains("Đảng viên làm giàu"))));
        // Author & Date
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("HÀ SỸ ĐỒNG") && s.text.contains("18/10/2026")),
            _ => false,
        }));
        // Sapo
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("Tạo điều kiện khuyến khích")),
            _ => false,
        }));
        // Full Article Body
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("Đoạn văn thân bài thứ nhất")),
            _ => false,
        }));
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("Đoạn văn thân bài thứ hai")),
            _ => false,
        }));
        // Article links (bullet •)
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::ListItem { bullet: '•', spans, .. } if spans.iter().any(|s| s.text.contains("Tin tức thời sự")))));
        // Main menu navigation links (bullet ▸)
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::ListItem { bullet: '▸', spans, .. } if spans.iter().any(|s| s.text.contains("Thời sự")))));
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::ListItem { bullet: '▸', spans, .. } if spans.iter().any(|s| s.text.contains("Kinh doanh")))));
    }

    #[test]
    fn test_reader_portal_extracts_article_links_without_fake_article() {
        let html = r#"
            <html>
            <body>
                <header class="header">
                    <nav class="main-menu">
                        <a href="/news">News</a>
                        <a href="/tech">Tech</a>
                    </nav>
                </header>
                <div class="modal_introduce">
                    <div class="popup content">
                        <p>Special offer subscribe today for premium member perks and bonus stars!</p>
                        <p>Enjoy distraction free reading and exclusive content discounts.</p>
                    </div>
                </div>
                <div class="news-feed">
                    <div class="card">
                        <a href="/breaking-cyber-defense-record-1002610.htm">Massive breakthrough in microkernel formal verification announced</a>
                    </div>
                    <div class="card">
                        <a href="/next-gen-cellos-operating-system-2026.htm">CellOS reaches Tier 1 verified security certification milestone</a>
                    </div>
                </div>
            </body>
            </html>
        "#;
        let parsed = crate::parser::html::parse_html(html);
        let extracted = extract_article(&parsed.arena, "https://news.portal/", "Portal");
        // No fake article from the popup!
        assert!(!extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("Special offer")),
            _ => false,
        }));
        // No heading "Danh sách liên kết trên trang"
        assert!(!extracted.nodes.iter().any(|node| matches!(node, DocNode::Heading { level: 2, text } if text.contains("Danh sách liên kết"))));
        // Article links extracted!
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::ListItem { spans, .. } if spans.iter().any(|s| s.text.contains("Massive breakthrough in microkernel")))));
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::ListItem { spans, .. } if spans.iter().any(|s| s.text.contains("CellOS reaches Tier 1")))));
    }

    #[test]
    fn test_reader_extracts_json_ld_and_htmldate_cascade() {
        let html = r#"
            <html>
            <head>
                <script type="application/ld+json">
                {
                    "@context": "https://schema.org",
                    "@type": "NewsArticle",
                    "headline": "Formal Verification of CellOS Microkernel Achieved",
                    "datePublished": "2026-10-19T08:00:00Z",
                    "author": {
                        "@type": "Person",
                        "name": "Dr. Alan Turing"
                    },
                    "description": "Researchers have mathematically proven complete isolation of domains."
                }
                </script>
            </head>
            <body>
                <div class="content-body">
                    <p>The mathematical proof guarantees that no capability leaks can occur across process boundaries.</p>
                    <p>This milestone represents the culmination of five years of rigorous development.</p>
                </div>
            </body>
            </html>
        "#;
        let parsed = crate::parser::html::parse_html(html);
        let extracted = extract_article(&parsed.arena, "https://cellos.org/news/verification", "Blog");
        assert!(extracted.title.contains("Formal Verification of CellOS"));
        // Author and normalized date in byline
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("Dr. Alan Turing") && s.text.contains("19/10/2026")),
            _ => false,
        }));
        // Sapo
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("mathematically proven complete isolation")),
            _ => false,
        }));
    }

    #[test]
    fn test_reader_extracts_date_from_url_pattern() {
        let html = r#"
            <html>
            <body>
                <article>
                    <h1>Mars Rover Finds Ancient Fossilized Microbes</h1>
                    <p>Geological core samples gathered from Jezero Crater reveal microscopic structures consistent with ancient biotic activity.</p>
                    <p>Detailed isotopic spectroscopy will confirm the biological origins over the coming weeks.</p>
                </article>
            </body>
            </html>
        "#;
        let parsed = crate::parser::html::parse_html(html);
        let extracted = extract_article(&parsed.arena, "https://science.org/2026/05/18/mars-fossils.html", "Science");
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("18/05/2026")),
            _ => false,
        }));
    }

    #[test]
    fn test_reader_native_form_extraction() {
        let html = r#"
            <html>
            <body>
                <header><img class="logo" src="/logo.png" alt="Google"></header>
                <h1>Search the Web</h1>
                <form action="/search" method="GET">
                    <label>Search query</label>
                    <input type="search" name="q" placeholder="Type keywords...">
                    <input type="submit" value="Google Search">
                </form>
                <nav>
                    <a href="/about">About Google</a>
                </nav>
            </body>
            </html>
        "#;
        let parsed = crate::parser::html::parse_html(html);
        let extracted = extract_article(&parsed.arena, "https://google.com/", "Google");
        // Forms extracted into native DocNode::Input
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::Input { input_type, name, placeholder, .. } if input_type == "search" && name == "q" && placeholder == "Type keywords...")));
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::Input { input_type, value, .. } if input_type == "submit" && value == "Google Search")));
    }

    #[test]
    fn test_reader_list_catalog_product_and_forum_extraction() {
        let html = r#"
            <html>
            <body>
                <h1>Technology Gadgets</h1>
                <div class="product-list">
                    <div class="product-card">
                        <a href="/item/logitech-mx3" class="title">Logitech MX Master 3S Mouse</a>
                        <span class="price">1.890.000 đ</span>
                        <p class="desc">Ergonomic quiet click mouse with 8K DPI sensor.</p>
                    </div>
                    <div class="product-card">
                        <a href="/item/keychron-k2" class="title">Keychron K2 Wireless Keyboard</a>
                        <span class="price">1.650.000 đ</span>
                        <p class="desc">Compact 75% mechanical keyboard with RGB backlighting.</p>
                    </div>
                </div>
                <div class="pagination">
                    <a href="/category/tech?p=2" class="page-link">Trang sau »</a>
                </div>
            </body>
            </html>
        "#;
        let parsed = crate::parser::html::parse_html(html);
        let extracted = extract_article(&parsed.arena, "https://store.test/category/tech", "Store");
        // Extracted list items with titles, prices, excerpts
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::ListItem { spans, .. } if spans.iter().any(|s| s.text == "Logitech MX Master 3S Mouse"))));
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::Paragraph { spans } if spans.iter().any(|s| s.text.contains("1.890.000 đ")))));
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::Paragraph { spans } if spans.iter().any(|s| s.text.contains("Ergonomic quiet click mouse")))));
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::ListItem { spans, .. } if spans.iter().any(|s| s.text == "Keychron K2 Wireless Keyboard"))));
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::Paragraph { spans } if spans.iter().any(|s| s.text.contains("1.650.000 đ")))));
        // Pagination links
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("Trang sau »") && s.link.is_some()),
            _ => false,
        }));
    }

    #[test]
    fn test_reader_link_density_penalty_disqualifies_sidebar() {
        let html = r#"
            <html>
            <body>
                <aside class="sidebar-links">
                    <p><a href="/a">Link A is very long text for navigation</a></p>
                    <p><a href="/b">Link B is very long text for navigation</a></p>
                    <p><a href="/c">Link C is very long text for navigation</a></p>
                    <p><a href="/d">Link D is very long text for navigation</a></p>
                </aside>
                <main class="prose">
                    <h1>Real Deep Analysis Article</h1>
                    <p>This is a real prose article with sentence structure, commas, and periods. It explains the inner workings of operating systems in depth.</p>
                    <p>Furthermore, the design principles of microkernels emphasize minimal privilege, compartmentalized faults, and formal guarantees.</p>
                </main>
            </body>
            </html>
        "#;
        let parsed = crate::parser::html::parse_html(html);
        let extracted = extract_article(&parsed.arena, "https://os.test/analysis", "OS Review");
        assert_eq!(extracted.title, "Real Deep Analysis Article [Reader]");
        // Main content is extracted
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("This is a real prose article")),
            _ => false,
        }));
        // Sidebar links are not extracted as article body
        assert!(!extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("Link A is very long text")),
            _ => false,
        }));
    }

    #[test]
    fn test_reader_forum_threads_extraction() {
        let html = r#"
            <html>
            <body>
                <header><img class="brand-logo" src="/logo.png" alt="vOz Forum"></header>
                <h1>Chuyện trò linh tinh</h1>
                <div class="threads-list">
                    <div class="thread-item">
                        <a href="/t/danh-gia-cellos-g2.1001" class="title">[Review] Đánh giá CellOS G2 trên Intel x86</a>
                        <span class="author">dmin</span>
                        <time class="date">19/10/2026</time>
                        <span class="reply-count">42 replies</span>
                    </div>
                    <div class="thread-item">
                        <a href="/t/chia-se-kinh-nghiem-viet-kernel.1002" class="title">[Thảo luận] Kinh nghiệm viết microkernel bằng Rust</a>
                        <span class="author">rustacean</span>
                        <time class="date">18/10/2026</time>
                        <span class="reply-count">128 replies</span>
                    </div>
                </div>
            </body>
            </html>
        "#;
        let parsed = crate::parser::html::parse_html(html);
        let extracted = extract_article(&parsed.arena, "https://voz.test/f/chuyen-tro-linh-tinh", "Forum");
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::ListItem { spans, .. } if spans.iter().any(|s| s.text.contains("[Review] Đánh giá CellOS G2")))));
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("dmin") && s.text.contains("19/10/2026") && s.text.contains("42 replies")),
            _ => false,
        }));
        assert!(extracted.nodes.iter().any(|node| matches!(node, DocNode::ListItem { spans, .. } if spans.iter().any(|s| s.text.contains("Kinh nghiệm viết microkernel")))));
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("rustacean") && s.text.contains("128 replies")),
            _ => false,
        }));
    }

    #[test]
    fn test_reader_multi_page_article_stitching() {
        let html = r#"
            <html>
            <head>
                <link rel="next" href="/article/page-2.html" />
            </head>
            <body>
                <article>
                    <h1>The Future of Microkernels - Part 1</h1>
                    <p>In this first part, we explore capability security and single address spaces.</p>
                </article>
            </body>
            </html>
        "#;
        let parsed = crate::parser::html::parse_html(html);
        let extracted = extract_article(&parsed.arena, "https://cellos.org/article/page-1.html", "Article");
        assert!(extracted.nodes.iter().any(|n| matches!(n, DocNode::ListItem { spans, .. } if spans.iter().any(|s| s.text.contains("Đọc tiếp trang tiếp theo") && s.link.as_deref() == Some("https://cellos.org/article/page-2.html")))));
    }
    #[test]
    fn test_reader_portal_list_cards_preserve_links_on_title_sapo_and_thumbnail() {
        let html = r#"
            <html>
            <body>
                <div class="news-list">
                    <div class="card">
                        <img src="/thumb1.jpg" alt="Photo">
                        <h3><a href="/post-101.html">Tiêu đề bài viết số 1</a></h3>
                        <p class="summary">Đoạn tóm tắt nội dung bài viết thứ nhất hiển thị trên trang chủ.</p>
                        <span class="date">20/10/2026</span>
                    </div>
                    <div class="card">
                        <img src="/thumb2.jpg" alt="Photo">
                        <h3><a href="/post-102.html">Tiêu đề bài viết số 2</a></h3>
                        <p class="summary">Đoạn tóm tắt nội dung bài viết thứ hai hiển thị trên trang chủ.</p>
                        <span class="date">20/10/2026</span>
                    </div>
                </div>
            </body>
            </html>
        "#;
        let parsed = crate::parser::html::parse_html(html);
        let extracted = extract_article(&parsed.arena, "https://example.com/category/news", "News Portal");

        // Title link
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::ListItem { spans, .. } => spans.iter().any(|s| s.text.contains("Tiêu đề bài viết số 1") && s.link == Some(String::from("https://example.com/post-101.html"))),
            _ => false,
        }));

        // Sapo summary link
        assert!(extracted.nodes.iter().any(|node| match node {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.text.contains("Đoạn tóm tắt") && s.link == Some(String::from("https://example.com/post-101.html"))),
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
