# Phase 01 — DSL Reactive Bindings

## Overview

| | |
|---|---|
| **Priority** | High |
| **Status** | Complete ✅ |
| **Stage** | G2 Wave 1 |
| **Crate** | `tools/vi-compiler` only — no `libs/viui` changes |
| **Parallel** | P02, P03, P04 (zero shared files) |

Replace `Expr::Raw(String)` with a typed `Expr` AST enum. Parser emits typed nodes; codegen
derives reactive `Signal::map()` chains from `SelfProp` references instead of text substitution.
Eliminates false positives and allows compile-time type inference in generated code.

---

## Key Insights

- Current `ast.rs`: `pub enum Expr { Raw(RawExpr) }` where `RawExpr = String`.
- `desugar_prop_refs` does text-replacement — mis-fires on string literals containing prop names.
- `eval.rs` already has `TypedExpr` enum (7 variants from P04). Unused for Expr resolution.
- `codegen.rs` calls `emit_builder_call(prop, expr)` which matches on raw strings.
- Tests run with `--target x86_64-pc-windows-msvc` (workspace default = riscv64 bare-metal).

---

## Requirements

### Functional
1. `Expr` AST variants: `Literal(Literal)`, `Ident(String)`, `SelfProp(String)`, `BinOp`,
   `Ternary`, `Interpolated(Vec<InterpPart>)`, `FnCall(String, Vec<Expr>)`.
2. Parser: `parse_expr()` → `Expr` (recursive descent, operator precedence).
3. `compile_expr(expr: &Expr, ctx: ExprCtx) -> String` in `eval.rs` → Rust source.
4. `SelfProp("x")` in static `build()` context → `*x.get()` (local var, NOT `self.x`).
5. `BinOp(SelfProp("a"), Add, SelfProp("b"))` → `*a.get() + *b.get()`.
6. For reactive props: if expr contains any `SelfProp`, codegen emits a `Signal::map()` chain
   rather than a one-time computed value.
7. `compile_error!` message includes source line number and expression text.

### Non-functional
- All 16 existing codegen tests continue to pass.
- New: ≥6 `compile_expr` unit tests.
- `cargo test -p vi-compiler --target x86_64-pc-windows-msvc` — all green.

---

## Architecture

```
.vi source
    │
    ▼  parse_expr()
ast::Expr (typed enum)
    │
    ▼  compile_expr(expr, ctx)
eval.rs
    │  ctx=ExprCtx::BuildFn  → *prop.get()
    │  ctx=ExprCtx::Reactive → Signal::map(|v| ...) chain
    ▼
Rust source string  →  codegen.rs emits into generated file
```

`ExprCtx` enum:
```rust
pub enum ExprCtx {
    BuildFn,   // inside static build() — props are local Signal<T> vars
    Reactive,  // at struct level — needs Signal::map chain
}
```

### `Expr` enum (ast.rs)
```rust
pub enum Expr {
    Literal(Literal),
    Ident(String),
    SelfProp(String),
    BinOp(Box<Expr>, BinOpKind, Box<Expr>),
    Unary(UnaryOp, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Interpolated(Vec<InterpPart>),
    FnCall(String, Vec<Expr>),
}
pub enum Literal    { Bool(bool), Int(i64), Float(f64), Str(String) }
pub enum BinOpKind  { Add, Sub, Mul, Div, Rem, Eq, Ne, Lt, Le, Gt, Ge, And, Or }
pub enum UnaryOp    { Not, Neg }
pub enum InterpPart { Lit(String), Expr(Box<Expr>) }
```

### `parse_expr()` (parser.rs)
Recursive descent with precedence levels:
1. `parse_ternary` → `cond ? then : else`
2. `parse_or` → `||`
3. `parse_and` → `&&`
4. `parse_eq` → `==` / `!=`
5. `parse_cmp` → `<` / `>` / `<=` / `>=`
6. `parse_add` → `+` / `-`
7. `parse_mul` → `*` / `/` / `%`
8. `parse_unary` → `!` / `-`
9. `parse_primary` → literal / ident / `self.prop` / `fn(args)` / `"text {expr}"` / `(expr)`

### `compile_expr()` (eval.rs)
```rust
pub fn compile_expr(expr: &Expr, ctx: ExprCtx) -> String {
    match expr {
        Expr::Literal(Literal::Bool(b))  => b.to_string(),
        Expr::Literal(Literal::Int(n))   => n.to_string(),
        Expr::Literal(Literal::Float(f)) => format!("{f}_f32"),
        Expr::Literal(Literal::Str(s))   => format!("{s:?}"),
        Expr::SelfProp(name) => match ctx {
            ExprCtx::BuildFn => format!("*{name}.get()"),
            ExprCtx::Reactive => format!("{name}"),  // referenced in map closure
        },
        Expr::BinOp(l, op, r) => format!(
            "({} {} {})", compile_expr(l, ctx), op.as_str(), compile_expr(r, ctx)
        ),
        ...
    }
}
```

Reactive map chain (when `SelfProp` refs found):
```rust
// DSL: text: "{speed} rpm"
// Generates:
let text = speed.map(|speed| alloc::format!("{speed} rpm"));
```

---

## Related Code Files

| File | Action |
|------|--------|
| `tools/vi-compiler/src/ast.rs` | **Modify** — replace `Expr::Raw` with typed enum |
| `tools/vi-compiler/src/parser.rs` | **Modify** — add `parse_expr()` recursive descent |
| `tools/vi-compiler/src/eval.rs` | **Modify** — add `compile_expr()`, `ExprCtx` |
| `tools/vi-compiler/src/codegen.rs` | **Modify** — use `compile_expr()` instead of string desugar |
| `tools/vi-compiler/tests/codegen_tests.rs` | **Modify** — add ≥6 new tests |
| `tools/vi-compiler/tests/parser_tests.rs` | **Modify** (or create) — expr parse tests |

---

## Implementation Steps

1. Add `Expr`, `Literal`, `BinOpKind`, `UnaryOp`, `InterpPart` enums to `ast.rs`.
   Keep old `RawExpr` behind `#[deprecated]` to aid migration, remove at step 4.
2. Add `parse_expr()` + helpers to `parser.rs`. Property value parsing calls `parse_expr()`.
3. Add `ExprCtx` enum + `compile_expr()` to `eval.rs`.
4. Update `codegen.rs`: replace `desugar_prop_refs()` with `compile_expr()` calls.
   Update `emit_builder_call()` to take `&Expr` instead of `&str`.
5. Update all existing parser tests to check typed `Expr` nodes (not raw strings).
6. Add new `compile_expr` unit tests (bool/int/float/str literals, SelfProp, BinOp, Interpolated).
7. Run `cargo test -p vi-compiler --target x86_64-pc-windows-msvc` — all green.
8. Run `cargo check --workspace` to confirm nothing broken in viui lib.

---

## Todo

- [x] Add typed `Expr` enum to `ast.rs`
- [x] Implement `parse_expr()` recursive descent in `parser.rs`
- [x] Add `ExprCtx` + `compile_expr()` to `eval.rs`
- [x] Update `codegen.rs` to use typed Expr
- [x] Write ≥6 new `compile_expr` tests
- [x] Update parser tests to check typed nodes
- [x] `cargo test -p vi-compiler --target x86_64-pc-windows-msvc` all pass

---

## Success Criteria

1. `Expr::Raw(String)` removed; compiler errors for any code still using it.
2. `SelfProp` in computed property → `Signal::map()` in generated Rust (verified by test).
3. `"hello {name}"` interpolation → `name.map(|name| format!("hello {name}"))`.
4. All 16 existing codegen tests pass unchanged.
5. 6+ new tests green.

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Parser ambiguity (ternary vs assignment) | Ternary is right-assoc; no assignment in DSL |
| `SelfProp` in non-reactive context emits wrong code | `ExprCtx` enum prevents mixing |
| Breaking existing `.vi` files | All existing tests must pass; parser stays backward-compatible |

---

## Security Considerations

vi-compiler runs at build time on trusted `.vi` source files. Generated Rust code is
type-checked by the Rust compiler before compilation — no runtime injection risk.

---

## Evidence

**Completion verified (2026-06-09):**

```
cargo test -p vi-compiler --target x86_64-pc-windows-msvc
   Compiling vi-compiler v0.2.0
    Finished test [unoptimized + debuginfo] target(s) in 8.34s
     Running unittests src/lib.rs
running 59 tests
...
test result: ok. 59 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

**Implementation summary:**
- ✅ `Expr` enum with 7 typed variants (Literal, Ident, SelfProp, BinOp, Unary, Ternary, Interpolated, FnCall)
- ✅ `parse_expr()` recursive descent with proper operator precedence
- ✅ `compile_expr(expr, ctx)` in eval.rs handles both BuildFn and Reactive contexts
- ✅ `SelfProp` refs generate `Signal::map()` chains in codegen (verified in generated code)
- ✅ All 16 existing codegen tests pass unchanged
- ✅ 6+ new tests added for typed expression compilation
- ✅ Parser fixes: StringLit re-quoting in raw expressions (test regex_in_string)
- ✅ No breaking changes to `libs/viui` — vi-compiler only

**Note:** Pre-existing errors in flex_box.rs (from P02 parallel work) do not affect P01 scope.

---

## Next Steps

After P01: P10 in old numbering = done. Next: codegen can handle DSL `for` loop with reactive
item source (`for item in items_signal { ... }` → `ListView::new(items_signal)`).
