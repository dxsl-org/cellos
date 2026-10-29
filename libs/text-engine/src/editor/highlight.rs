//! Lexical highlighting for `.toml`, `.sh` and `.rs` only (contract §3.3).
//!
//! Styling, never validation: spans cover byte ranges of one line's text and
//! nothing here changes text or evaluates anything. The only state carried
//! between lines is a Rust block comment or a TOML triple-quoted string.

use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Toml,
    Shell,
    Rust,
}

impl Lang {
    /// By case-sensitive filename suffix; everything else is plain text.
    pub fn for_path(path: &str) -> Option<Self> {
        let name = path.rsplit('/').next().unwrap_or(path);
        if name.ends_with(".toml") {
            Some(Self::Toml)
        } else if name.ends_with(".sh") {
            Some(Self::Shell)
        } else if name.ends_with(".rs") {
            Some(Self::Rust)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Comment,
    String,
    Number,
    Keyword,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

/// State carried into the next line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Carry {
    #[default]
    None,
    RustBlockComment,
    /// Inside a TOML multi-line string closed by three of this quote.
    TomlTriple(u8),
}

const RUST_KEYWORDS: [&str; 19] = [
    "fn", "let", "mut", "pub", "use", "mod", "struct", "enum", "impl", "trait",
    "if", "else", "match", "for", "while", "loop", "return", "const", "static",
];

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// End of a quoted string starting at `start` (the quote); unterminated runs
/// to the end of the line.
fn string_end(bytes: &[u8], start: usize, escapes: bool) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if escapes => i += 2,
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn number_end(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
        i += 1;
    }
    if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
        i += 1;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
            i += 1;
        }
    }
    i
}

fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| from + p)
}

/// `'x'` or a short escape like `'\n'` starting at `i`; `None` for a lifetime.
fn char_literal_end(text: &str, i: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(i + 1) == Some(&b'\\') {
        return (i + 3..bytes.len().min(i + 12)).find(|&j| bytes[j] == b'\'');
    }
    let after = i + 1 + text.get(i + 1..)?.chars().next()?.len_utf8();
    (bytes.get(after) == Some(&b'\'')).then_some(after)
}

/// Spans of one line, appended to `out`; returns the carry for the next line.
pub fn highlight(lang: Lang, carry: Carry, text: &str, out: &mut Vec<Span>) -> Carry {
    let bytes = text.as_bytes();
    let mut push = |start: usize, end: usize, kind| {
        if start < end {
            out.push(Span { start, end, kind });
        }
    };
    let mut i = 0;
    match carry {
        Carry::None => {}
        Carry::RustBlockComment => match find(bytes, 0, b"*/") {
            Some(end) => { push(0, end + 2, Kind::Comment); i = end + 2; }
            None => { push(0, bytes.len(), Kind::Comment); return carry; }
        },
        Carry::TomlTriple(q) => match find(bytes, 0, &[q; 3]) {
            Some(end) => { push(0, end + 3, Kind::String); i = end + 3; }
            None => { push(0, bytes.len(), Kind::String); return carry; }
        },
    }
    while i < bytes.len() {
        let b = bytes[i];
        let word_start = i == 0 || !is_ident(bytes[i - 1]);
        let comment_hash = b == b'#' && (lang == Lang::Toml || i == 0 || bytes[i - 1].is_ascii_whitespace());
        match (lang, b) {
            (Lang::Rust, b'/') if bytes.get(i + 1) == Some(&b'/') => { push(i, bytes.len(), Kind::Comment); break; }
            (Lang::Rust, b'/') if bytes.get(i + 1) == Some(&b'*') => match find(bytes, i + 2, b"*/") {
                Some(end) => { push(i, end + 2, Kind::Comment); i = end + 2; }
                None => { push(i, bytes.len(), Kind::Comment); return Carry::RustBlockComment; }
            },
            (Lang::Rust, b'"') => { let end = string_end(bytes, i, true); push(i, end, Kind::String); i = end; }
            (Lang::Rust, b'\'') => match char_literal_end(text, i) {
                Some(end) => { push(i, end + 1, Kind::String); i = end + 1; }
                None => i += 1,
            },
            (Lang::Toml | Lang::Shell, b'#') if comment_hash => { push(i, bytes.len(), Kind::Comment); break; }
            (Lang::Toml, b'"' | b'\'') if bytes.get(i..i + 3) == Some(&[b; 3][..]) => match find(bytes, i + 3, &[b; 3]) {
                Some(end) => { push(i, end + 3, Kind::String); i = end + 3; }
                None => { push(i, bytes.len(), Kind::String); return Carry::TomlTriple(b); }
            },
            (Lang::Toml | Lang::Shell, b'"') => { let end = string_end(bytes, i, true); push(i, end, Kind::String); i = end; }
            (Lang::Toml | Lang::Shell, b'\'') => { let end = string_end(bytes, i, false); push(i, end, Kind::String); i = end; }
            (Lang::Toml | Lang::Rust, b'0'..=b'9') if word_start => { let end = number_end(bytes, i); push(i, end, Kind::Number); i = end; }
            (Lang::Rust, _) if word_start && is_ident(b) => {
                let end = (i..bytes.len()).find(|&j| !is_ident(bytes[j])).unwrap_or(bytes.len());
                if RUST_KEYWORDS.contains(&&text[i..end]) {
                    push(i, end, Kind::Keyword);
                }
                i = end;
            }
            _ => i += text[i..].chars().next().map_or(1, char::len_utf8),
        }
    }
    Carry::None
}
