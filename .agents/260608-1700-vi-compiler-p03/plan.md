# ViUI v2 P03 — vi-compiler: Lexer + Parser → AST

**Plan ID**: 260608-1700-vi-compiler-p03  
**Stage**: G2  
**Priority**: P1 — required before code generation (P04) + build.rs integration (P05)  
**Created**: 2026-06-08  
**Depends on**: P01/P02 NOT required — standalone std crate  
**Design Brief**: [.agents/brainstorms/260608-viui-nextgen-architecture.md](../brainstorms/260608-viui-nextgen-architecture.md)

---

## Mục tiêu

Xây dựng `vi-compiler` — Rust tool chuyển `.vi` DSL (Slint-compatible syntax) thành AST:

1. **Lexer** — tokenize `.vi` source → `Vec<Token>` (Slint-like tokens)
2. **AST types** — `ViFile`, `Component`, `Element`, `PropertyDecl`, `Binding`, `Expr::Raw`
3. **Parser** — recursive descent, produces AST from token stream
4. **Error types** — `LexError`, `ParseError` với span information

**P03 scope**: Structure only — expressions parsed as `Expr::Raw(String)` (opaque source text).  
**P04 scope** (future): Expression evaluator + Rust codegen from AST.

---

## Crate Strategy

`vi-compiler` là **standalone std crate** tách khỏi ViCell workspace:
- Workspace default target = `riscv64gc-unknown-none-elf` → conflict nếu join
- Build tool chạy trên dev machine (host), không phải embedded
- `tools/vi-compiler/Cargo.toml` + `src/` (không add vào workspace root `Cargo.toml`)
- Build: `cargo run --manifest-path tools/vi-compiler/Cargo.toml`

---

## Grammar Subset (P03)

Dựa trên counter.vi example từ design brief:

```slint
component Counter {
    in-out property <int> count: 0;

    VerticalLayout {
        padding: 16px;
        Text { text: "Count: \{count}"; color: #ffffff; }
        Button {
            text: "Increment";
            clicked => { count += 1; }
        }
    }
}
```

```
file           = import* component*
import         = 'import' STRING ';'
component      = 'component' IDENT '{' component_item* '}'
component_item = property_decl | element | callback_decl
property_decl  = visibility? 'property' '<' IDENT '>' IDENT (':' expr_raw)? ';'
visibility     = 'in' | 'out' | 'in-out' | 'private'
element        = IDENT '{' element_item* '}'
element_item   = binding | callback_binding | element
binding        = IDENT ':' expr_raw ';'
callback_binding = IDENT '=>' '{' expr_raw '}' ';'?
expr_raw       = (any token except unmatched '}' or ';')* -- P03: opaque
```

---

## Phase Table

| Phase | File | Nội dung | Status |
|-------|------|----------|--------|
| P01 | [phase-01-crate-lexer.md](phase-01-crate-lexer.md) | Crate setup + token types + Lexer | ✅ Done |
| P02 | [phase-02-ast-parser.md](phase-02-ast-parser.md) | AST types + recursive descent Parser + tests | ✅ Done |

P02 cần token types từ P01.

---

## Files Created

```
tools/vi-compiler/
├── Cargo.toml          (standalone, NOT in workspace)
└── src/
    ├── lib.rs           pub mod declarations
    ├── token.rs         TokenKind, Token<'src>, Span
    ├── lexer.rs         Lexer<'src> — hand-written scanner
    ├── ast.rs           ViFile, Component, Element, PropertyDecl, Expr
    ├── parser.rs        recursive descent Parser<'src>
    └── error.rs         LexError, ParseError
```

---

## Key Design Decisions

### No external parser deps (YAGNI)
Hand-written recursive descent — Slint's grammar is LL(k≤2), no ambiguity requiring LR. Full error message control, no build-time codegen.

### `in-out` lexing
Lexer emits `in`, `-`, `out` as separate tokens. Parser combines into `Visibility::InOut` when it sees `in` `-` `out` sequence. No special lexer hack needed.

### Expr::Raw — P03 opaque expressions
```rust
pub struct RawExpr {
    pub text: String,  // source text of the expression, trimmed
    pub span: Span,
}
pub enum Expr { Raw(RawExpr) }
```
P04 will add `Expr::Literal(Literal)`, `Expr::BinOp(...)`, etc. and implement an evaluator.

### Error recovery
P03: fail-fast (return `ParseError` on first error). P04 can add recovery if needed.

### Test fixtures
`tests/fixtures/counter.vi` — the canonical counter example.  
Unit tests: `lexer.rs` → verify token types; `parser.rs` → verify AST structure.
