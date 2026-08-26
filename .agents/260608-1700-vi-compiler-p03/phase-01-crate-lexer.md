# Phase 01 — Crate Setup + Lexer

**Plan**: [plan.md](plan.md)  
**Status**: Planned  
**Estimated**: 1–2 hours

---

## Context

Create the `tools/vi-compiler/` standalone crate and implement the hand-written lexer that tokenizes `.vi` source into a `Vec<Token>`.

This is a pure-std crate with no dependencies. Runs on the developer's host machine.

---

## Requirements

### `token.rs` — Token types

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ── Structural keywords ───────────────────────────────────────
    KwComponent,  // component
    KwProperty,   // property
    KwImport,     // import
    KwExport,     // export
    KwIn,         // in
    KwOut,        // out
    KwPrivate,    // private
    KwIf,         // if
    KwElse,       // else
    KwFor,        // for
    KwReturn,     // return
    KwTrue,       // true
    KwFalse,      // false
    KwAnimations, // animate (future — lex but parser will reject for now)

    // ── Identifiers ───────────────────────────────────────────────
    Ident,        // Counter, VerticalLayout, count, padding, ...

    // ── Literals ─────────────────────────────────────────────────
    IntLit,       // 42
    FloatLit,     // 3.14
    StringLit,    // "text" (with \{} interpolation as raw text)
    ColorLit,     // #ffffff, #fff
    LengthLit,    // 16px, 8em, 2rem
    PercentLit,   // 50%

    // ── Operators / Punctuation ───────────────────────────────────
    Plus,         // +
    Minus,        // -
    Star,         // *
    Slash,        // /
    Assign,       // =
    Arrow,        // =>
    Colon,        // :
    Semicolon,    // ;
    Comma,        // ,
    Dot,          // .
    Bang,         // !
    Question,     // ?
    Ampersand,    // &
    Pipe,         // |
    Lt,           // <
    Gt,           // >
    LtEq,         // <=
    GtEq,         // >=
    EqEq,         // ==
    BangEq,       // !=
    And,          // &&
    Or,           // ||
    PlusEq,       // +=
    MinusEq,      // -=

    // ── Brackets ─────────────────────────────────────────────────
    LBrace,       // {
    RBrace,       // }
    LParen,       // (
    RParen,       // )
    LBracket,     // [
    RBracket,     // ]

    // ── Trivia (consumed, not emitted by default) ─────────────────
    LineComment,  // // ...
    BlockComment, // /* ... */
    Whitespace,   // spaces, tabs, newlines

    // ── End ──────────────────────────────────────────────────────
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line:  u32,
    pub col:   u32,
    pub start: u32,  // byte offset from source start
    pub len:   u32,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,   // owned slice of source (for standalone use without lifetime)
    pub span: Span,
}
```

**Note**: Using `String` (not `&'src str`) for `Token::text` — owned, simpler lifetime management. Lexer makes one pass; cloning text is acceptable for a build tool.

---

## Lexer logic — `lexer.rs`

```rust
pub struct Lexer<'src> {
    src:  &'src str,
    pos:  usize,      // byte offset
    line: u32,
    col:  u32,
}

impl<'src> Lexer<'src> {
    pub fn new(src: &'src str) -> Self { ... }

    /// Tokenize the full source, skipping trivia (whitespace + comments).
    /// Returns error if an unexpected character is found.
    pub fn tokenize(src: &'src str) -> Result<Vec<Token>, LexError> { ... }
}
```

### Lexer rules (in priority order)

1. Skip whitespace + comments
2. `//` → line comment until `\n`
3. `/* ... */` → block comment (nested not supported)
4. `"..."` → StringLit (handle `\"` escape, preserve raw content including `\{}`)
5. `#` followed by hex chars → ColorLit (`#fff`, `#ffffff`, `#ffffffff`)
6. Digit → IntLit / FloatLit / LengthLit / PercentLit  
   - `42px`, `8em`, `2rem` → LengthLit  
   - `50%` → PercentLit  
   - `3.14` → FloatLit  
   - `42` → IntLit
7. Alpha or `_` → Ident, then keyword check via match on text
8. Multi-char operators: `=>`, `<=`, `>=`, `==`, `!=`, `&&`, `||`, `+=`, `-=`
9. Single-char operators/punctuation

### Keyword map

```rust
fn keyword(s: &str) -> Option<TokenKind> {
    match s {
        "component" => Some(TokenKind::KwComponent),
        "property"  => Some(TokenKind::KwProperty),
        "import"    => Some(TokenKind::KwImport),
        "export"    => Some(TokenKind::KwExport),
        "in"        => Some(TokenKind::KwIn),
        "out"       => Some(TokenKind::KwOut),
        "private"   => Some(TokenKind::KwPrivate),
        "if"        => Some(TokenKind::KwIf),
        "else"      => Some(TokenKind::KwElse),
        "for"       => Some(TokenKind::KwFor),
        "return"    => Some(TokenKind::KwReturn),
        "true"      => Some(TokenKind::KwTrue),
        "false"     => Some(TokenKind::KwFalse),
        "animate"   => Some(TokenKind::KwAnimations),
        _           => None,
    }
}
```

---

## Files to Create

| File | Action |
|------|--------|
| `tools/vi-compiler/Cargo.toml` | **CREATE** — standalone crate, no workspace |
| `tools/vi-compiler/src/lib.rs` | **CREATE** — pub mod token; pub mod lexer; pub mod ast; pub mod parser; pub mod error; |
| `tools/vi-compiler/src/token.rs` | **CREATE** — TokenKind, Token, Span |
| `tools/vi-compiler/src/lexer.rs` | **CREATE** — Lexer, tokenize() |
| `tools/vi-compiler/src/error.rs` | **CREATE** — LexError, ParseError (stubs for now) |

---

## `Cargo.toml` for vi-compiler

```toml
[package]
name    = "vi-compiler"
version = "0.1.0"
edition = "2021"
description = "ViUI .vi DSL compiler — lexer + parser → AST"

[[bin]]
name = "vi-compiler"
path = "src/main.rs"

[lib]
name = "vi_compiler"
path = "src/lib.rs"

# No external deps — hand-written lexer/parser, pure std
[dependencies]
```

---

## Implementation Steps

1. Create `tools/vi-compiler/Cargo.toml`
2. Create `tools/vi-compiler/src/lib.rs` with mod declarations
3. Create `tools/vi-compiler/src/error.rs` (stubs)
4. Create `tools/vi-compiler/src/token.rs` (TokenKind, Token, Span)
5. Create `tools/vi-compiler/src/lexer.rs` (Lexer, tokenize())
6. Create minimal `tools/vi-compiler/src/main.rs` (reads file, prints tokens)
7. `cargo test --manifest-path tools/vi-compiler/Cargo.toml` — lexer unit tests pass

---

## Success Criteria

- [ ] `cargo build --manifest-path tools/vi-compiler/Cargo.toml` compiles
- [ ] `cargo test --manifest-path tools/vi-compiler/Cargo.toml` passes
- [ ] Tokenizing counter.vi example produces correct token types:
  - `component` → KwComponent
  - `Counter` → Ident
  - `in-out` → KwIn, Minus, KwOut (three tokens)
  - `16px` → LengthLit
  - `#ffffff` → ColorLit
  - `"Count: \{count}"` → StringLit (raw content preserved)
  - `=>` → Arrow

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `in-out` hyphen ambiguity | Lex as 3 tokens; parser combines |
| String interpolation `\{}` | Lexer treats `\` as escape, preserves `{count}` literally in StringLit text |
| Length unit suffix (`px`, `em`) | Lookahead: after digit sequence, check alpha suffix |
| `<` / `>` in property type vs comparison | Parser context handles it (type context only in `property <type>`) |
