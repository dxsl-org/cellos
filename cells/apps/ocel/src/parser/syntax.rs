// SPDX-License-Identifier: MIT
//! Small lexical highlighter: no grammar, execution, or external runtime.
//! State lives for one code block, preserving multiline strings and comments.

extern crate alloc;
use crate::doc::StyledSpan;
use crate::draw::{theme, Color};
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Rust,
    C,
    Json,
    Toml,
}

impl Language {
    pub fn from_name(name: &str) -> Option<Self> {
        match name.split_whitespace().next().unwrap_or("") {
            "rust" | "rs" => Some(Self::Rust),
            "c" | "h" | "cpp" | "c++" | "cc" | "cxx" | "hpp" => Some(Self::C),
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            _ => None,
        }
    }
    pub fn from_url(url: &str) -> Option<Self> {
        let path = url.split(['?', '#']).next().unwrap_or(url);
        let ext = path.rsplit('.').next()?.to_ascii_lowercase();
        Self::from_name(&ext)
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::C => "c",
            Self::Json => "json",
            Self::Toml => "toml",
        }
    }
}

#[derive(Clone, Copy)]
enum State {
    Normal,
    BlockComment(usize),
    String { quote: u8, triple: bool },
    RawString(usize),
}

pub struct Highlighter {
    language: Option<Language>,
    state: State,
}

impl Highlighter {
    pub fn new(name: &str) -> Self {
        Self {
            language: Language::from_name(name),
            state: State::Normal,
        }
    }

    pub fn line(&mut self, text: &str) -> Vec<StyledSpan> {
        let Some(language) = self.language else {
            return alloc::vec![StyledSpan::code(text)];
        };
        let bytes = text.as_bytes();
        let mut spans = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let start = i;
            let color;
            match self.state {
                State::BlockComment(mut depth) => {
                    while i < bytes.len() {
                        if language == Language::Rust && bytes[i..].starts_with(b"/*") {
                            depth += 1;
                            i += 2;
                        } else if bytes[i..].starts_with(b"*/") {
                            depth -= 1;
                            i += 2;
                            if depth == 0 {
                                break;
                            }
                        } else {
                            i += char_len(text, i);
                        }
                    }
                    self.state = if depth == 0 {
                        State::Normal
                    } else {
                        State::BlockComment(depth)
                    };
                    color = theme::TEXT_MUTED;
                }
                State::String { quote, triple } => {
                    i = consume_string(text, i, quote, triple, &mut self.state);
                    color = Color::rgb(166, 227, 161);
                }
                State::RawString(hashes) => {
                    i = consume_raw(text, i, hashes, &mut self.state);
                    color = Color::rgb(166, 227, 161);
                }
                State::Normal => {
                    if matches!(language, Language::Rust | Language::C)
                        && bytes[i..].starts_with(b"//")
                        || language == Language::Toml && bytes[i] == b'#'
                    {
                        i = bytes.len();
                        color = theme::TEXT_MUTED;
                    } else if matches!(language, Language::Rust | Language::C)
                        && bytes[i..].starts_with(b"/*")
                    {
                        self.state = State::BlockComment(1);
                        i += 2;
                        color = theme::TEXT_MUTED;
                    } else if let Some((prefix, hashes)) = (language == Language::Rust)
                        .then(|| raw_open(&bytes[i..]))
                        .flatten()
                    {
                        i += prefix;
                        self.state = State::RawString(hashes);
                        i = consume_raw(text, i, hashes, &mut self.state);
                        color = Color::rgb(166, 227, 161);
                    } else if bytes[i] == b'"' || language == Language::Toml && bytes[i] == b'\'' {
                        let quote = bytes[i];
                        let triple =
                            language == Language::Toml && bytes[i..].starts_with(&[quote; 3]);
                        i += if triple { 3 } else { 1 };
                        self.state = State::String { quote, triple };
                        i = consume_string(text, i, quote, triple, &mut self.state);
                        color = Color::rgb(166, 227, 161);
                    } else if let Some(end) = (bytes[i] == b'\''
                        && matches!(language, Language::Rust | Language::C))
                    .then(|| char_literal_end(text, i))
                    .flatten()
                    {
                        i = end;
                        color = Color::rgb(166, 227, 161);
                    } else if bytes[i].is_ascii_digit() {
                        i += 1;
                        while i < bytes.len()
                            && (bytes[i].is_ascii_alphanumeric()
                                || bytes[i] == b'_'
                                || bytes[i] == b'.'
                                    && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
                        {
                            i += 1;
                        }
                        color = Color::rgb(250, 179, 135);
                    } else if identifier_start(text, i) {
                        i += char_len(text, i);
                        while i < bytes.len() && identifier_continue(text, i) {
                            i += char_len(text, i);
                        }
                        color = if keyword(language, &text[start..i]) {
                            theme::ACCENT_BLUE
                        } else {
                            theme::TEXT_PRIMARY
                        };
                    } else {
                        i += char_len(text, i);
                        color = theme::TEXT_PRIMARY;
                    }
                }
            }
            append(&mut spans, &text[start..i], color);
        }
        spans
    }
}

fn char_len(text: &str, i: usize) -> usize {
    text[i..].chars().next().unwrap().len_utf8()
}
fn identifier_start(text: &str, i: usize) -> bool {
    let c = text[i..].chars().next().unwrap();
    c == '_' || c.is_alphabetic()
}
fn identifier_continue(text: &str, i: usize) -> bool {
    let c = text[i..].chars().next().unwrap();
    c == '_' || c.is_alphanumeric()
}
fn append(spans: &mut Vec<StyledSpan>, text: &str, color: Color) {
    if let Some(last) = spans.last_mut() {
        if last.color == Some(color) {
            last.text.push_str(text);
            return;
        }
    }
    let mut span = StyledSpan::plain(text);
    span.color = Some(color);
    // Code-block background belongs to the layout box, not per-token pills.
    spans.push(span);
}
fn raw_open(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut i = if bytes.starts_with(b"br") {
        2
    } else if bytes.starts_with(b"r") {
        1
    } else {
        return None;
    };
    let start = i;
    while bytes.get(i) == Some(&b'#') {
        i += 1;
    }
    (bytes.get(i) == Some(&b'"')).then_some((i + 1, i - start))
}
fn consume_raw(text: &str, mut i: usize, hashes: usize, state: &mut State) -> usize {
    let b = text.as_bytes();
    while i < b.len() {
        if b[i] == b'"'
            && b.get(i + 1..i + 1 + hashes)
                .is_some_and(|s| s.iter().all(|c| *c == b'#'))
        {
            *state = State::Normal;
            return i + 1 + hashes;
        }
        i += char_len(text, i);
    }
    i
}
fn consume_string(text: &str, mut i: usize, quote: u8, triple: bool, state: &mut State) -> usize {
    let b = text.as_bytes();
    // A backslash-newline escapes the newline, not the next line's first character.
    let mut escaped = false;
    while i < b.len() {
        if escaped {
            escaped = false;
            i += char_len(text, i);
            continue;
        }
        if b[i] == b'\\' && quote != b'\'' {
            escaped = true;
            i += 1;
            continue;
        }
        if b[i] == quote && (!triple || b[i..].starts_with(&[quote; 3])) {
            *state = State::Normal;
            return i + if triple { 3 } else { 1 };
        }
        i += char_len(text, i);
    }
    *state = State::String { quote, triple };
    i
}
fn char_literal_end(text: &str, start: usize) -> Option<usize> {
    let b = text.as_bytes();
    let mut i = start + 1;
    if b.get(i) == Some(&b'\\') {
        i += 1;
        if b.get(i) == Some(&b'u') && b.get(i + 1) == Some(&b'{') {
            i += 2;
            while b
                .get(i)
                .is_some_and(|c| c.is_ascii_hexdigit() || *c == b'_')
            {
                i += 1;
            }
            if b.get(i) != Some(&b'}') {
                return None;
            }
            i += 1;
        } else if b.get(i) == Some(&b'x') {
            i += 1;
            for _ in 0..2 {
                if !b.get(i)?.is_ascii_hexdigit() {
                    return None;
                }
                i += 1;
            }
        } else {
            if i >= b.len() {
                return None;
            }
            i += char_len(text, i);
        }
    } else {
        if i >= b.len() || b[i] == b'\'' {
            return None;
        }
        i += char_len(text, i);
    }
    (b.get(i) == Some(&b'\'')).then_some(i + 1)
}
fn keyword(language: Language, word: &str) -> bool {
    match language {
        Language::Rust => matches!(
            word,
            "as" | "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "crate"
                | "dyn"
                | "else"
                | "enum"
                | "extern"
                | "false"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "Self"
                | "static"
                | "struct"
                | "super"
                | "trait"
                | "true"
                | "type"
                | "unsafe"
                | "use"
                | "where"
                | "while"
        ),
        Language::C => matches!(
            word,
            "auto"
                | "bool"
                | "break"
                | "case"
                | "char"
                | "class"
                | "const"
                | "constexpr"
                | "continue"
                | "default"
                | "do"
                | "double"
                | "else"
                | "enum"
                | "extern"
                | "false"
                | "float"
                | "for"
                | "if"
                | "inline"
                | "int"
                | "long"
                | "namespace"
                | "new"
                | "nullptr"
                | "private"
                | "public"
                | "return"
                | "short"
                | "signed"
                | "sizeof"
                | "static"
                | "struct"
                | "switch"
                | "template"
                | "true"
                | "typedef"
                | "union"
                | "unsigned"
                | "using"
                | "virtual"
                | "void"
                | "volatile"
                | "while"
        ),
        Language::Json => matches!(word, "true" | "false" | "null"),
        Language::Toml => matches!(word, "true" | "false"),
    }
}

#[cfg(test)]
#[path = "syntax_tests.rs"]
mod tests;
