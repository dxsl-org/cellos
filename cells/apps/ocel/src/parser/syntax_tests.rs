// SPDX-License-Identifier: MIT
use super::*;

fn colour_at(spans: &[StyledSpan], byte: usize) -> Color {
    let mut end = 0;
    for span in spans {
        end += span.text.len();
        if byte < end {
            return span.color.unwrap();
        }
    }
    panic!("byte not present in highlighted line");
}
fn preserved(spans: &[StyledSpan], original: &str) {
    let joined: alloc::string::String = spans.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(joined, original);
}

#[test]
fn rust_nested_comments_survive_line_boundaries_and_resume_code() {
    let mut h = Highlighter::new("rust");
    let first = "let x = 1; /* outer /* nested";
    let spans = h.line(first);
    preserved(&spans, first);
    assert_eq!(colour_at(&spans, 0), theme::ACCENT_BLUE);
    assert_eq!(
        colour_at(&spans, first.find("outer").unwrap()),
        theme::TEXT_MUTED
    );
    let second = "*/ still outer */ fn next() {} // tail";
    let spans = h.line(second);
    preserved(&spans, second);
    assert_eq!(
        colour_at(&spans, second.find("still").unwrap()),
        theme::TEXT_MUTED
    );
    assert_eq!(
        colour_at(&spans, second.find("fn").unwrap()),
        theme::ACCENT_BLUE
    );
    assert_eq!(
        colour_at(&spans, second.find("tail").unwrap()),
        theme::TEXT_MUTED
    );
}

#[test]
fn raw_string_requires_matching_hashes_and_does_not_parse_comments() {
    let mut h = Highlighter::new("rs");
    let first = "let s = r##\"// fn \"#";
    let spans = h.line(first);
    preserved(&spans, first);
    assert_eq!(
        colour_at(&spans, first.find("fn").unwrap()),
        Color::rgb(166, 227, 161)
    );
    let second = "/* text */\"##; let n = 42;";
    let spans = h.line(second);
    preserved(&spans, second);
    assert_eq!(colour_at(&spans, 0), Color::rgb(166, 227, 161));
    assert_eq!(
        colour_at(&spans, second.find("let").unwrap()),
        theme::ACCENT_BLUE
    );
}

#[test]
fn lifetimes_unicode_and_escaped_characters_preserve_text() {
    let text = "fn f<'a>(c: &'a str) { let café = '\\n'; let λ = 'é'; }";
    let spans = Highlighter::new("rust").line(text);
    preserved(&spans, text);
    assert_eq!(
        colour_at(&spans, text.find("'a").unwrap()),
        theme::TEXT_PRIMARY
    );
    assert_eq!(
        colour_at(&spans, text.find("'é'").unwrap()),
        Color::rgb(166, 227, 161)
    );
    for text in ["'", "'\\", "'é", "let 字 = 1;"] {
        preserved(&Highlighter::new("rust").line(text), text);
    }
}

#[test]
fn strings_take_precedence_over_comment_delimiters() {
    for language in ["rust", "c", "json", "toml"] {
        let text = "\"escaped \\\" // # /*\" true";
        let spans = Highlighter::new(language).line(text);
        preserved(&spans, text);
        assert_eq!(
            colour_at(&spans, text.find("//").unwrap()),
            Color::rgb(166, 227, 161)
        );
        assert_eq!(
            colour_at(&spans, text.find("true").unwrap()),
            theme::ACCENT_BLUE
        );
    }
}

#[test]
fn toml_multiline_literal_string_and_comment_have_separate_scopes() {
    let mut h = Highlighter::new("toml");
    let first = "key = '''literal # not comment";
    preserved(&h.line(first), first);
    let second = "still literal''' # comment";
    let spans = h.line(second);
    preserved(&spans, second);
    assert_eq!(colour_at(&spans, 0), Color::rgb(166, 227, 161));
    assert_eq!(
        colour_at(&spans, second.find("#").unwrap()),
        theme::TEXT_MUTED
    );
}

#[test]
fn unknown_languages_and_unlabelled_blocks_are_not_guessed() {
    for language in ["", "python", "text"] {
        let text = "fn test() { /* plain */ }";
        let spans = Highlighter::new(language).line(text);
        preserved(&spans, text);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].color, Some(theme::ACCENT_CYAN));
    }
}

#[test]
fn source_urls_accept_cpp_and_query_suffixes_but_not_plain_text() {
    assert_eq!(
        Language::from_url("file:///src/MAIN.RS?x=1#L2"),
        Some(Language::Rust)
    );
    assert_eq!(
        Language::from_url("file:///src/main.hpp"),
        Some(Language::C)
    );
    assert_eq!(
        Language::from_url("file:///data/config.toml"),
        Some(Language::Toml)
    );
    assert_eq!(Language::from_url("file:///data/example.txt"), None);
}

#[test]
fn code_search_crosses_token_boundaries_without_duplicate_box_matches() {
    let mut doc = crate::doc::Document::new();
    doc.nodes = alloc::vec![crate::doc::DocNode::CodeBlock {
        lang: alloc::string::String::from("rust"),
        lines: alloc::vec![
            alloc::string::String::from("let café = 42;"),
            alloc::string::String::from("let café = 43;"),
        ],
    }];
    doc.compute_layout(800);
    let offset = doc.layout_boxes[0].y_offset;
    assert_eq!(doc.search("LET café ="), alloc::vec![offset]);
    assert_eq!(doc.search("= 42;"), alloc::vec![offset]);
    assert!(doc.search("43; missing").is_empty());
    assert!(doc.search("  ").is_empty());
}
