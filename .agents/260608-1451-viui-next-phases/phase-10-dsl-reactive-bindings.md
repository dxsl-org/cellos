# Phase 10 — DSL Reactive Bindings + Advanced Codegen

**Status:** Complete  
**Stage:** G2  
**Priority:** Medium  
**Estimate:** 2-3 ngày  
**Depends on:** Phase 04 (DSL widget registry v2)

---

## Context

Phase 01 `desugar_prop_ref()` là simple string replace: `self.X` → `*self.X.get()`.

Problems:
1. False positives trong string literals: `"self.value"` → `"*self.value.get()"` (wrong)
2. Reactive bindings không work: `text: parent.color` → should create `Signal::map` not static copy
3. `Expr` AST còn là `Raw(String)` — P04 codegen chưa có proper Expr variants

---

## Part A — Expr AST proper types

`tools/vi-compiler/src/ast.rs` cần:

```rust
#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Literal),
    Ident(String),                              // bare identifier
    SelfProp(String),                           // self.X
    BinOp(Box<Expr>, BinOpKind, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Interpolated(Vec<InterpPart>),              // "hello {self.name}"
    FnCall(String, Vec<Expr>),                  // min(a, b)
}

#[derive(Debug, Clone)]
pub enum Literal {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Color(u32),   // #RRGGBB
}
```

`eval.rs` hiện tại đã có `TypedExpr` — đây là refactor/alignment, không rebuild.

---

## Part B — Reactive binding desugaring

```vi
// .vi source (property reference binding):
component Counter {
    property theme_color: color: parent.accent_color
    Text { color: self.theme_color }
}
```

Generated Rust (reactive):
```rust
// In __vi_generated_Counter struct:
theme_color: Signal<Color>,

// In constructor:
let theme_color = Signal::new(*parent.accent_color.get());
parent.accent_color.subscribe({
    let theme_color = theme_color.clone();
    move |v| { theme_color.set(*v); }
});
```

vs. static (current): `let theme_color = Signal::new(*parent.accent_color.get());` (no update).

### Detection rule

If default expr is `SelfProp(name)` or `Ident(name)` → emit reactive `Signal::map` binding.  
If default expr is `Literal(...)` → emit static init.  
If complex expr (BinOp, Ternary) → emit `Signal::map` with computed closure.

---

## Part C — Animation in .vi files

G1 defer: animation required explicit Rust code. G2 add `.vi` syntax:

```vi
Slider {
    value: self.speed
    animate value { duration: 200ms; easing: ease-out }
}
```

Codegen emits `AnimatedSignal` wrapping the signal:

```rust
let speed_animated = AnimatedSignal::new(*self.speed.get());
self.speed.subscribe({
    let anim = speed_animated.clone();
    move |v| { anim.animate_to(*v, 200); }
});
```

This is complex — defer to G2 Phase 10b if needed.

---

## Related Code Files

| File | Action |
|------|--------|
| `tools/vi-compiler/src/ast.rs` | MODIFY — proper Expr enum variants |
| `tools/vi-compiler/src/parser.rs` | MODIFY — parse Expr properly |
| `tools/vi-compiler/src/eval.rs` | MODIFY — align TypedExpr with new Expr |
| `tools/vi-compiler/src/codegen.rs` | MODIFY — reactive binding desugaring |
| `tools/vi-compiler/tests/codegen_tests.rs` | MODIFY — reactive binding tests |

---

## Success Criteria

- `text: self.name` in .vi file → generates reactive `Signal::map` binding
- String literal `text: "Hello"` → static `Signal::new("Hello".into())`
- Binop expression `value: self.a + self.b` → `Signal::new(*a.get() + *b.get())` (G2: reactive)
- No false-positives in string literal desugaring
- Existing 36 codegen tests pass
