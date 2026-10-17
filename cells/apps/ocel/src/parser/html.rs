// SPDX-License-Identifier: MIT
//! HTML subset parser & Tree Builder for Ocel.
//!
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use dom_arena::{DocumentArena, NodeData, NodeId};

use crate::doc::{DocNode, StyledSpan};
use crate::draw::theme;

pub enum ScriptSource {
    Inline(String),
    External(String),
}

pub struct HtmlOutput {
    pub nodes: Vec<DocNode>,
    pub scripts: Vec<ScriptSource>,
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
fn tokenize_html(input: &str) -> (Vec<HtmlToken>, Vec<ScriptSource>) {
    let mut tokens = Vec::new();
    let mut scripts = Vec::new();
    let mut chars = input.chars().peekable();
    let mut template_depth=0usize;

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
            let mut quote = None;
            while let Some(ch) = chars.next() {
                if quote == Some(ch) {
                    quote = None;
                } else if quote.is_none() && matches!(ch, '"' | '\'') {
                    quote = Some(ch);
                } else if ch == '>' && quote.is_none() {
                    break;
                }
                tag_content.push(ch);
            }

            let trimmed_tag = tag_content.trim();
            if let Some(rest) = trimmed_tag.strip_prefix('/') {
                let name = rest.trim().to_ascii_lowercase();
                if name=="template"{template_depth=template_depth.saturating_sub(1);}
                tokens.push(HtmlToken::EndTag { name });
            } else {
                let self_closing = trimmed_tag.ends_with('/');
                let tag_body = if self_closing {
                    trimmed_tag.trim_end_matches('/').trim()
                } else {
                    trimmed_tag
                };

                let name_end = tag_body.find(char::is_whitespace).unwrap_or(tag_body.len());
                let name = tag_body[..name_end].to_ascii_lowercase();
                let attributes = parse_attributes(&tag_body[name_end..]);
                if name=="template"&&!self_closing{template_depth+=1;}

                // If this is a <script> tag, capture the script body directly
                if name == "script" && !self_closing {
                    let mut script_body = String::new();
                    while let Some(sc) = chars.next() {
                        if sc=='<' {
                            if let Some(count)=crate::budget::rawtext_close_chars(chars.clone(),"script") {
                                for _ in 0..count{chars.next();}
                                break;
                            }
                        }
                        script_body.push(sc);
                    }
                    let kind = attributes.iter().find(|(key, _)| key == "type").map(|(_, value)| value.as_str()).unwrap_or("");
                    if template_depth==0&&matches!(kind, "" | "text/javascript" | "application/javascript") {
                        if let Some((_, src)) = attributes.iter().find(|(key, _)| key == "src") {
                            scripts.push(ScriptSource::External(src.clone()));
                        } else {
                            scripts.push(ScriptSource::Inline(script_body));
                        }
                    }
                    continue;
                }
                if name == "style" && !self_closing {
                    tokens.push(HtmlToken::StartTag { name: name.clone(), attributes, self_closing });
                    let mut body = String::new();
                    while let Some(ch) = chars.next() {
                        if ch=='<' {
                            if let Some(count)=crate::budget::rawtext_close_chars(chars.clone(),"style") {
                                for _ in 0..count{chars.next();}
                                break;
                            }
                        }
                        body.push(ch);
                    }
                    tokens.push(HtmlToken::Text(body));
                    tokens.push(HtmlToken::EndTag { name });
                    continue;
                }
                if matches!(name.as_str(),"textarea"|"title"|"xmp"|"iframe"|"noembed"|"noframes")&&!self_closing {
                    tokens.push(HtmlToken::StartTag{name:name.clone(),attributes,self_closing});
                    let mut body=String::new();
                    while let Some(c)=chars.next() {
                        if c=='<' {
                            if let Some(count)=crate::budget::rawtext_close_chars(chars.clone(),&name) {
                                for _ in 0..count{chars.next();}break;
                            }
                        }
                        body.push(c);
                    }
                    tokens.push(HtmlToken::Text(decode_entities(&body)));tokens.push(HtmlToken::EndTag{name});continue;
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
            // Preserve DOM whitespace; inline layout performs CSS collapsing.
            tokens.push(HtmlToken::Text(decoded));
        }
    }

    (tokens, scripts)
}

fn parse_attributes(input: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = input.chars().peekable();
    while chars.peek().is_some() {
        while chars.peek().is_some_and(|ch| ch.is_whitespace()) { chars.next(); }
        let mut key = String::new();
        while chars.peek().is_some_and(|ch| !ch.is_whitespace() && *ch != '=') {
            key.push(chars.next().unwrap());
        }
        if key.is_empty() { break; }
        while chars.peek().is_some_and(|ch| ch.is_whitespace()) { chars.next(); }
        let mut value = String::new();
        if chars.peek() == Some(&'=') {
            chars.next();
            while chars.peek().is_some_and(|ch| ch.is_whitespace()) { chars.next(); }
            if chars.peek().is_some_and(|ch| matches!(ch, '"' | '\'')) {
                let quote = chars.next().unwrap();
                for ch in chars.by_ref() {
                    if ch == quote { break; }
                    value.push(ch);
                }
            } else {
                while chars.peek().is_some_and(|ch| !ch.is_whitespace()) {
                    value.push(chars.next().unwrap());
                }
            }
        }
        let key = key.to_ascii_lowercase();
        if !out.iter().any(|(existing, _)| existing == &key) {
            out.push((key, decode_entities(&value)));
        }
    }
    out
}

/// Parse HTML text into a `DocumentArena` and renderable `DocNode`s.
#[allow(dead_code)]
pub fn parse_html(input: &str) -> HtmlOutput {
    parse_html_with_base(input, None)
}

pub fn parse_html_with_base(input: &str, base_url: Option<&str>) -> HtmlOutput {
    parse_html_with_base_checked(input, base_url).expect("HTML document budget")
}

pub fn parse_html_with_base_checked(input: &str, base_url: Option<&str>) -> Result<HtmlOutput, String> {
    crate::budget::validate_html(input)?;
    let (tokens, scripts) = tokenize_html(input);
    let mut arena = DocumentArena::new();
    let mut tag_stack: Vec<(String, NodeId)> = alloc::vec![(String::from("root"), arena.root)];

    for tok in tokens {
        if arena.nodes.len()>=crate::budget::MAX_DOM_NODES {return Err(String::from("HTML exceeds DOM node budget"));}
        match tok {
            HtmlToken::StartTag {
                name,
                attributes,
                self_closing,
            } => {
                if !self_closing && tag_stack.len()>crate::budget::MAX_DOM_DEPTH {return Err(String::from("HTML exceeds DOM depth budget"));}
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

    let mut images = Vec::new();
    let mut css = String::new();
    for node in &arena.nodes {
        if node.tag() == Some("img") { if let Some(src) = node.get_attribute("src") { images.push(src); } }
        if node.tag() == Some("style") {
            css.push_str(&arena.get_text_content(node.id));
        }
        if let Some(style) = node.get_attribute("style") { css.push_str("x{"); css.push_str(style); css.push('}'); }
    }
    crate::budget::validate_css(&css)?;
    super::validate_images(base_url, images.into_iter())?;
    let nodes = arena_to_doc_nodes_with_base(&arena, base_url);
    Ok(HtmlOutput {
        nodes,
        scripts,
        arena,
    })
}

pub fn arena_to_doc_nodes_with_base(arena: &DocumentArena, base_url: Option<&str>) -> Vec<DocNode> {
    let mut doc_nodes = Vec::new();
    render_children(arena, arena.root, &mut doc_nodes, base_url);
    for node in &arena.nodes {
        if node.tag()!=Some("img")||doc_nodes.iter().any(|asset|matches!(asset,DocNode::DomImage{node_id,..} if *node_id==node.id)){continue;}
        if let (Some(base),Some(src))=(base_url,node.get_attribute("src")) {
            if let Ok(bytes)=crate::resources::load_binary(base,src) {
                if let Some(image)=crate::image::decode(&bytes) {doc_nodes.push(DocNode::DomImage{node_id:node.id,width:image.width,height:image.height,pixels:alloc::rc::Rc::new(image.pixels)});}
            }
        }
    }
    doc_nodes
}

fn render_children(arena: &DocumentArena, parent_id: NodeId, out: &mut Vec<DocNode>, base_url: Option<&str>) {
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
                        "p" => {
                            let mut spans = Vec::new();
                            collect_inline_spans(
                                arena, child_id, &mut spans, false, false, false, None,
                            );
                            if !spans.is_empty() {
                                out.push(DocNode::Paragraph { spans });
                            }
                        }
                        "div" | "section" | "article" | "header" | "footer" | "main" => {
                            let mut has_element_children = false;
                            let mut scan = child.first_child;
                            while let Some(sc_id) = scan {
                                if let Some(sc_node) = arena.get(sc_id) {
                                    if matches!(sc_node.data, NodeData::Element { .. }) {
                                        has_element_children = true;
                                        break;
                                    }
                                }
                                scan = arena.get(sc_id).and_then(|n| n.next_sibling);
                            }
                            if has_element_children {
                                render_children(arena, child_id, out, base_url);
                            } else {
                                let mut spans = Vec::new();
                                collect_inline_spans(
                                    arena, child_id, &mut spans, false, false, false, None,
                                );
                                if !spans.is_empty() {
                                    out.push(DocNode::Paragraph { spans });
                                }
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
                        "img" => {
                            let src = child.get_attribute("src");
                            let alt = child.get_attribute("alt").unwrap_or("Image");
                            let mut loaded = false;
                            if let (Some(src_val), Some(base)) = (src, base_url) {
                                if !base.is_empty() {
                                    if let Ok(bytes) = crate::resources::load_binary(base, src_val) {
                                        if let Some(img) = crate::image::decode(&bytes) {
                                            out.push(DocNode::DomImage {
                                                node_id: child_id,
                                                width: img.width,
                                                height: img.height,
                                                pixels: alloc::rc::Rc::new(img.pixels),
                                            });
                                            loaded = true;
                                        }
                                    }
                                }
                            }
                            if !loaded {
                                let mut span = StyledSpan::plain(&alloc::format!("[🖼 {}]", alt));
                                span.color = Some(theme::TEXT_MUTED);
                                out.push(DocNode::Paragraph {
                                    spans: alloc::vec![span],
                                });
                            }
                        }
                        "input" => {
                            let itype = child.get_attribute("type").unwrap_or("text").to_ascii_lowercase();
                            let name = child.get_attribute("name").unwrap_or("").into();
                            let value = child.get_attribute("value").unwrap_or("").into();
                            let placeholder = child.get_attribute("placeholder").unwrap_or("").into();
                            let checked = child.get_attribute("checked").is_some();
                            out.push(DocNode::Input {
                                input_type: itype,
                                name,
                                value,
                                placeholder,
                                checked,
                                node_id: Some(child_id),
                            });
                        }
                        "textarea" => {
                            let name = child.get_attribute("name").unwrap_or("").into();
                            let placeholder = child.get_attribute("placeholder").unwrap_or("").into();
                            let value = arena.get_text_content(child_id).trim().into();
                            out.push(DocNode::Input {
                                input_type: String::from("textarea"),
                                name,
                                value,
                                placeholder,
                                checked: false,
                                node_id: Some(child_id),
                            });
                        }
                        "form" => {
                            render_children(arena, child_id, out, base_url);
                        }
                        _ => {
                            // Recursively render other elements
                            render_children(arena, child_id, out, base_url);
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
                    if tag == "img" {
                        let alt = attributes.iter().find(|(k, _)| k == "alt").map(|(_, v)| v.as_str()).unwrap_or("Image");
                        let mut span = StyledSpan::plain(&alloc::format!("[🖼 {}]", alt));
                        span.color = Some(theme::TEXT_MUTED);
                        out.push(span);
                        cur = arena.get(child_id).and_then(|n| n.next_sibling);
                        continue;
                    }
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

#[cfg(test)]
mod tokenizer_tests {
    use super::*;
    #[test]
    fn quoted_attributes_and_raw_styles_survive_tokenization() {
        let parsed = parse_html("<style>.a > .b { color: red; }</style><p class=\"a b\" title='x > y' data-x=\"a&amp;b\">hello</p>");
        let paragraph = parsed.arena.nodes.iter().find(|node| node.tag() == Some("p")).unwrap();
        assert_eq!(paragraph.get_attribute("class"), Some("a b"));
        assert_eq!(paragraph.get_attribute("title"), Some("x > y"));
        assert_eq!(paragraph.get_attribute("data-x"), Some("a&b"));
        let style = parsed.arena.nodes.iter().find(|node| node.tag() == Some("style")).unwrap();
        assert_eq!(parsed.arena.get_text_content(style.id), ".a > .b { color: red; }");
    }
    #[test]
    fn classic_scripts_preserve_source_order_without_executing_data_or_modules() {
        let parsed = parse_html("<script src='app.js'>ignored()</script><script>second()</script><script type='application/json'>{}</script><script type='module'>module()</script>");
        assert!(matches!(&parsed.scripts[0], ScriptSource::External(src) if src == "app.js"));
        assert!(matches!(&parsed.scripts[1], ScriptSource::Inline(src) if src == "second()"));
        assert_eq!(parsed.scripts.len(), 2);
    }
}

/// Resource discovery shares tokenization with the actual document parser.
pub fn resource_references_checked(input:&str)->Result<Vec<(String,bool)>,String> {
    crate::budget::validate_html(input)?;
    let (tokens,scripts)=tokenize_html(input);
    let mut refs=Vec::new();let mut images=0usize;let mut inert=0usize;
    let mut add=|url:String,image:bool|->Result<(),String>{
        if url.len()>2048{return Err(String::from("Resource URL exceeds byte budget"));}
        if image{images+=1;if images>crate::budget::MAX_IMAGES{return Err(String::from("Document exceeds image count budget"));}}
        if refs.len()>=64{return Err(String::from("Document exceeds resource count budget"));}
        refs.push((url,image));Ok(())
    };
    for token in tokens {match token {
        HtmlToken::StartTag{name,attributes,self_closing}=>{
            if name=="template" {if !self_closing{inert+=1;}continue;}
            if inert>0{continue;}
            if name=="img" {if let Some((_,url))=attributes.iter().find(|(name,_)|name=="src"){add(url.clone(),true)?;}}
            if name=="link"&&attributes.iter().any(|(name,value)|name=="rel"&&value.split_whitespace().any(|word|word.eq_ignore_ascii_case("stylesheet"))) {
                if let Some((_,url))=attributes.iter().find(|(name,_)|name=="href"){add(url.clone(),false)?;}
            }
        },
        HtmlToken::EndTag{name}=>{if name=="template"{inert=inert.saturating_sub(1);}},
        _=>{}
    }}
    for script in scripts {if let ScriptSource::External(url)=script{add(url,false)?;}}
    Ok(refs)
}
