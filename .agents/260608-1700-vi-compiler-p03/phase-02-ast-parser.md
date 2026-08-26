# Phase 02 — AST Types + Recursive Descent Parser

**Plan**: [plan.md](plan.md)  
**Depends on**: Phase 01 (TokenKind, Token, Span)  
**Status**: Planned  
**Estimated**: 2–3 hours

---

## Context

With the lexer producing a `Vec<Token>`, the parser builds a structural AST representing the `.vi` file. Expressions are stored as `Expr::Raw(String)` — the raw source text — to be evaluated in P04.

This gives us a clean separation: P03 understands **structure** (what components/elements/bindings exist); P04 understands **values** (what expressions mean).

---

## AST types — `ast.rs`

```rust
// ─── Top-level ─────────────────────────────────────────────────────────────

pub struct ViFile {
    pub imports:    Vec<Import>,
    pub components: Vec<Component>,
}

pub struct Import {
    pub path: String,
    pub span: Span,
}

// ─── Component ─────────────────────────────────────────────────────────────

pub struct Component {
    pub name:       String,
    pub properties: Vec<PropertyDecl>,
    pub callbacks:  Vec<CallbackDecl>,
    pub children:   Vec<Element>,
    pub span:       Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visibility {
    In,
    Out,
    InOut,
    Private,
}

pub struct PropertyDecl {
    pub visibility: Option<Visibility>,
    pub ty:         String,      // "int", "string", "bool", "color", "length"
    pub name:       String,
    pub default:    Option<Expr>,
    pub span:       Span,
}

pub struct CallbackDecl {
    pub name:   String,
    pub params: Vec<(String, String)>,  // (name, type)
    pub span:   Span,
}

// ─── Element ───────────────────────────────────────────────────────────────

pub struct Element {
    pub name:      String,               // VerticalLayout, Text, Button, ...
    pub bindings:  Vec<Binding>,
    pub callbacks: Vec<CallbackBinding>,
    pub children:  Vec<Element>,
    pub span:      Span,
}

pub struct Binding {
    pub property: String,
    pub value:    Expr,
    pub span:     Span,
}

pub struct CallbackBinding {
    pub name: String,
    pub body: String,  // raw source text of the callback body
    pub span: Span,
}

// ─── Expressions ───────────────────────────────────────────────────────────

/// P03: expressions stored as raw source text.
/// P04 will extend this enum with typed variants.
pub struct RawExpr {
    pub text: String,  // trimmed source text
    pub span: Span,
}

pub enum Expr {
    Raw(RawExpr),
    // P04 adds: Literal, Ident, BinOp, Ternary, Interpolated, FnCall, ...
}
```

---

## Parser logic — `parser.rs`

### Structure

```rust
pub struct Parser {
    tokens: Vec<Token>,
    pos:    usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self { ... }

    pub fn parse_file(&mut self) -> Result<ViFile, ParseError> {
        let mut imports    = Vec::new();
        let mut components = Vec::new();
        while !self.at_eof() {
            match self.peek_kind() {
                TokenKind::KwImport    => imports.push(self.parse_import()?),
                TokenKind::KwComponent => components.push(self.parse_component()?),
                _ => return Err(self.unexpected("import or component")),
            }
        }
        Ok(ViFile { imports, components })
    }
}
```

### Key parsing methods

```
parse_import()            → consume 'import' STRING ';'
parse_component()         → consume 'component' IDENT '{' component_items* '}'
parse_component_item()    → dispatch on peek: property_decl | element | callback_decl
parse_property_decl()     → visibility? 'property' '<' IDENT '>' IDENT (':' expr_raw)? ';'
parse_visibility()        → 'in' ('-' 'out')? | 'out' | 'private' | None
parse_callback_decl()     → 'callback' IDENT '(' params ')' ';'
parse_element()           → IDENT '{' element_items* '}'
parse_element_item()      → binding | callback_binding | element (lookahead on ':', '=>', '{')
parse_binding()           → IDENT ':' expr_raw ';'
parse_callback_binding()  → IDENT '=>' '{' raw_body '}'
parse_expr_raw()          → collect tokens until ';' or '}' (balanced braces)
parse_raw_body()          → collect tokens until matching '}'
```

### `in-out` visibility parsing

```rust
fn parse_visibility(&mut self) -> Option<Visibility> {
    match self.peek_kind() {
        TokenKind::KwIn => {
            self.advance(); // consume 'in'
            // Check for '-' followed by 'out'
            if self.peek_kind() == TokenKind::Minus
               && self.peek_kind_at(1) == TokenKind::KwOut {
                self.advance(); // consume '-'
                self.advance(); // consume 'out'
                Some(Visibility::InOut)
            } else {
                Some(Visibility::In)
            }
        }
        TokenKind::KwOut     => { self.advance(); Some(Visibility::Out)     }
        TokenKind::KwPrivate => { self.advance(); Some(Visibility::Private) }
        _ => None,
    }
}
```

### `parse_expr_raw()` — collect until terminator

```rust
fn parse_expr_raw(&mut self) -> Expr {
    let start = self.pos;
    let mut depth = 0i32;
    let mut parts = Vec::new();

    loop {
        match self.peek_kind() {
            TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => {
                depth += 1;
                parts.push(self.advance().text.clone());
            }
            TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => {
                if depth == 0 { break; }
                depth -= 1;
                parts.push(self.advance().text.clone());
            }
            TokenKind::Semicolon if depth == 0 => break,
            TokenKind::Eof => break,
            _ => parts.push(self.advance().text.clone()),
        }
    }

    let text = parts.join(" ").trim().to_string();
    let span = self.span_from(start);
    Expr::Raw(RawExpr { text, span })
}
```

### Element item disambiguation (lookahead)

`IDENT` followed by:
- `':'` → binding
- `'=>'` → callback binding  
- `'{'` → child element
- `'<'` could be a child element with no body check → use `IDENT '{'` pattern

```rust
fn parse_element_item(&mut self) -> Result<ElementItem, ParseError> {
    match (self.peek_kind(), self.peek_kind_at(1)) {
        // callback binding: IDENT =>
        (TokenKind::Ident, TokenKind::Arrow) => Ok(ElementItem::Callback(self.parse_callback_binding()?)),
        // binding: IDENT :
        (TokenKind::Ident, TokenKind::Colon) => Ok(ElementItem::Binding(self.parse_binding()?)),
        // child element: IDENT {
        (TokenKind::Ident, TokenKind::LBrace) => Ok(ElementItem::Child(self.parse_element()?)),
        (TokenKind::RBrace, _) => Err(ParseError::unexpected_close()),
        _ => Err(self.unexpected("binding, callback, or child element")),
    }
}
```

---

## Test fixtures — `tests/fixtures/counter.vi`

```slint
component Counter {
    in-out property <int> count: 0;

    VerticalLayout {
        padding: 16px;
        spacing: 8px;

        Text { text: "Count: \{count}"; color: #ffffff; }
        Button {
            text: "Increment";
            clicked => { count += 1; }
        }
    }
}
```

Expected AST:
- 1 Component named "Counter"
- 1 PropertyDecl: InOut, type="int", name="count", default=Raw("0")
- 1 Element child: "VerticalLayout" with:
  - Binding { padding: Raw("16px") }
  - Binding { spacing: Raw("8px") }
  - Child Element "Text" with bindings text + color
  - Child Element "Button" with binding text + callback clicked

---

## Files to Create

| File | Action |
|------|--------|
| `tools/vi-compiler/src/ast.rs` | **CREATE** |
| `tools/vi-compiler/src/parser.rs` | **CREATE** |
| `tools/vi-compiler/src/error.rs` | **MODIFY** — add ParseError variants |
| `tools/vi-compiler/tests/fixtures/counter.vi` | **CREATE** |
| `tools/vi-compiler/tests/parser_tests.rs` | **CREATE** |

---

## Implementation Steps

1. Create `tools/vi-compiler/src/ast.rs`
2. Complete `tools/vi-compiler/src/error.rs` with ParseError
3. Create `tools/vi-compiler/src/parser.rs`
4. Create `tests/fixtures/counter.vi`
5. Create `tests/parser_tests.rs` with integration test
6. `cargo test --manifest-path tools/vi-compiler/Cargo.toml` — all pass

---

## Success Criteria

- [ ] `cargo test` passes with 0 failures
- [ ] `counter.vi` parses to exactly 1 Component named "Counter"
- [ ] PropertyDecl: visibility=InOut, ty="int", name="count", default=Raw("0")
- [ ] Element hierarchy: Counter → VerticalLayout → [Text, Button]
- [ ] `clicked => { count += 1; }` → CallbackBinding { name: "clicked", body: "count += 1;" }
- [ ] Unknown token at top level → `ParseError::UnexpectedToken`

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Lookahead for element_item disambiguation | `peek_kind_at(1)` checks 2 tokens ahead — sufficient for LL(2) |
| `parse_expr_raw()` consuming too much | Balanced-brace depth counter + `;` sentinel |
| `CallbackBinding` body includes `{}` | `parse_raw_body()` collects between the outer `{ }`, returning inner text |
| Component body vs Element body | Both call same `parse_element_item()` logic — DRY |
