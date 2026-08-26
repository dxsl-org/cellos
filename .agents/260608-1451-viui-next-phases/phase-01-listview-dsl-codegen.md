# Phase 01 — ListView + DSL if/for Codegen

**Status:** Complete  
**Stage:** G1  
**Priority:** Critical  
**Estimate:** 2-3 ngày  
**Depends on:** P01-P03 embedded/robot readiness (DONE)

---

## Context Links

- [`libs/viui/src/node_widgets/`](../../../libs/viui/src/node_widgets/) — pattern: slider.rs, progress_bar.rs
- [`libs/viui/src/node_widgets.rs`](../../../libs/viui/src/node_widgets.rs) — registration
- [`tools/vi-compiler/src/ast.rs`](../../../tools/vi-compiler/src/ast.rs) — Element struct (flat, no If/For)
- [`tools/vi-compiler/src/parser.rs`](../../../tools/vi-compiler/src/parser.rs) — no if/for parsing yet
- [`tools/vi-compiler/src/codegen.rs`](../../../tools/vi-compiler/src/codegen.rs) — map_element chỉ có VBox/HBox/Text/Button
- [`tools/vi-compiler/tests/`](../../../tools/vi-compiler/tests/) — codegen_tests

---

## Overview

Hai deliverables độc lập:

1. **ListView widget** — scrollable list từ `Signal<Vec<String>>`, cần cho robot event log + task queue
2. **DSL if/for codegen** — conditional + loop trong `.vi` files compile ra valid Rust

Cả hai đều cần thiết cho Robot Dashboard Demo (P02).

**Gap quan trọng phát hiện khi đọc code:**
- `ast.rs`: `Element` là flat struct (`name`, `bindings`, `children`). KHÔNG có `If`/`For` variants.
- `parser.rs`: KHÔNG parse `if`/`for` keywords.
- `codegen.rs`: `map_element()` chỉ map 4 widget types (VBox/HBox/Text/Button). ProgressBar/Slider/ListView/TouchArea THIẾU.

P04 (DSL Widget Registry) sẽ fix map_element; phase này focus on ListView widget + if/for AST/parse/codegen.

---

## Part A — ListView Widget

### Architecture

```rust
// libs/viui/src/node_widgets/list_view.rs

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;

use crate::layout::Rect;
use crate::node::ViNode;
use crate::render_ctx::RenderCtx;
use crate::signal::{Signal, SubscriptionHandle};
use crate::event::{EventCtx, Response};

pub struct ListView {
    items:          Signal<Vec<String>>,
    selected:       Signal<Option<usize>>,
    on_select:      Option<Box<dyn Fn(usize)>>,
    item_height:    f32,
    scroll_offset:  Cell<f32>,
    bounds_cache:   Cell<Rect>,
    _subs:          Vec<SubscriptionHandle>,
}
```

**Builder pattern** (consistent với Slider/ProgressBar):
```rust
impl ListView {
    pub fn new(items: Signal<Vec<String>>) -> Self { ... }
    pub fn item_height(mut self, h: f32) -> Self { ... }
    pub fn selected(mut self, sel: Signal<Option<usize>>) -> Self { ... }
    pub fn on_select(mut self, f: impl Fn(usize) + 'static) -> Self { ... }
}
```

**Paint logic:**
1. Clip canvas đến bounds_cache rect
2. Compute visible range: `first = (scroll_offset / item_height) as usize`
3. Visible range end = first + (bounds.height / item_height).ceil() as usize + 1
4. For each visible item i:
   - item_y = bounds.y + i as f32 * item_height - scroll_offset
   - background: selected[i] → theme.accent (dim), else transparent
   - text: `cx.draw_text(Point{x: bounds.x + 4.0, y: item_y + 4.0}, &items[i], ...)`
5. Unclip

**Non-virtual G1**: render ALL items, clip hides off-screen. Safe cho n ≤ 200.
Virtual render defer đến P08.

**Event handling:**
- `Scroll { delta_y }` → scroll_offset += delta_y * 3.0, clamp [0, max_scroll]
- `MousePress { pos }` hoặc `TouchBegin { pos }` → hit-test item index → set selected, call on_select
- `TouchMove` với delta_y > threshold (8px) → scroll mode (không trigger select)

### Scroll offset clamping
```rust
fn max_scroll(items_len: usize, item_height: f32, bounds_height: f32) -> f32 {
    (items_len as f32 * item_height - bounds_height).max(0.0)
}
```

### Dirty propagation
Subscribe vào `items` signal: khi items thay đổi → mark dirty_rect = bounds_cache.  
Subscribe vào `selected` signal: same.  
Store handles trong `_subs` (auto-unsubscribe on drop = Law 8 compliant).

---

## Part B — DSL if/for Codegen

### Step 1: Extend AST (`ast.rs`)

Add `If` và `For` variants vào `Element`:

```rust
// Current:
pub struct Element {
    pub name:      String,
    pub bindings:  Vec<Binding>,
    pub callbacks: Vec<CallbackBinding>,
    pub children:  Vec<Element>,
    pub span:      Span,
}

// New approach: keep Element as struct, add ControlFlow enum for special nodes
// OR: make children Vec<ChildNode> where ChildNode is Element | If | For
```

**Recommended**: rename `Element` children to `Vec<Child>` where:

```rust
#[derive(Debug)]
pub enum Child {
    Element(Element),
    If { cond: String, body: Vec<Child>, span: Span },
    For { var: String, iter: String, body: Vec<Child>, span: Span },
}
```

Dùng `String` cho cond/iter (raw expression text) — đủ cho G1, P10 sẽ refine sang Expr.

Thay `children: Vec<Element>` → `children: Vec<Child>` trong Component và Element.

### Step 2: Parser (`parser.rs`)

Thêm `parse_child()` dispatcher:

```rust
fn parse_child(&mut self) -> Result<Child, ParseError> {
    match self.peek_kind() {
        TokenKind::KwIf  => self.parse_if_child(),
        TokenKind::KwFor => self.parse_for_child(),
        _                => Ok(Child::Element(self.parse_element()?)),
    }
}

fn parse_if_child(&mut self) -> Result<Child, ParseError> {
    let span = self.peek().span;
    self.expect(TokenKind::KwIf)?;
    // Read condition: everything until '{'
    let cond = self.read_until_lbrace()?;
    self.expect(TokenKind::LBrace)?;
    let mut body = Vec::new();
    while *self.peek_kind() != TokenKind::RBrace {
        body.push(self.parse_child()?);
    }
    self.expect(TokenKind::RBrace)?;
    Ok(Child::If { cond, body, span })
}

fn parse_for_child(&mut self) -> Result<Child, ParseError> {
    let span = self.peek().span;
    self.expect(TokenKind::KwFor)?;
    let var  = self.expect_ident()?;
    self.expect(TokenKind::KwIn)?;
    let iter = self.read_until_lbrace()?;
    self.expect(TokenKind::LBrace)?;
    let mut body = Vec::new();
    while *self.peek_kind() != TokenKind::RBrace {
        body.push(self.parse_child()?);
    }
    self.expect(TokenKind::RBrace)?;
    Ok(Child::For { var, iter, body, span })
}
```

Thêm `KwIf`, `KwFor`, `KwIn` vào `token.rs` / `lexer.rs` keyword table.

### Step 3: Codegen (`codegen.rs`)

`emit_child()` dispatcher thay cho `emit_element()`:

```rust
fn emit_child(out: &mut String, child: &Child, comp: &mut CompState) -> fmt::Result {
    match child {
        Child::Element(e) => emit_element(out, e, comp),
        Child::If { cond, body, .. } => {
            writeln!(out, "if {} {{", desugar_prop_ref(cond))?;
            for c in body { emit_child(out, c, comp)?; }
            writeln!(out, "}}")
        }
        Child::For { var, iter, body, .. } => {
            let w_counter = comp.widget_counter;
            comp.widget_counter += 1;
            // Vertical stacking: track running y inside loop
            writeln!(out, "{{")?;
            writeln!(out, "let mut __for_y_{w_counter} = 0.0f32;")?;
            writeln!(out, "for (_{var}_idx, {var}) in {}.iter().enumerate() {{", desugar_prop_ref(iter))?;
            for c in body { emit_child(out, c, comp)?; }
            writeln!(out, "__for_y_{w_counter} += item_height;")?;
            writeln!(out, "}}")?;
            writeln!(out, "}}")
        }
    }
}
```

`desugar_prop_ref(s)`: replace `self.X` → `*self.X.get()` (simple string replace cho G1).

### Tokens needed in lexer

```
"if"  → KwIf
"for" → KwFor
"in"  → KwIn
```

Thêm vào `lexer.rs` keyword map.

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/viui/src/node_widgets/list_view.rs` | CREATE |
| `libs/viui/src/node_widgets.rs` | MODIFY — add `pub mod list_view; pub use list_view::ListView;` |
| `libs/viui/src/lib.rs` | MODIFY — re-export ListView |
| `tools/vi-compiler/src/ast.rs` | MODIFY — add `Child` enum, update `children` fields |
| `tools/vi-compiler/src/token.rs` | MODIFY — add KwIf, KwFor, KwIn |
| `tools/vi-compiler/src/lexer.rs` | MODIFY — add keyword mappings |
| `tools/vi-compiler/src/parser.rs` | MODIFY — parse_child, parse_if_child, parse_for_child |
| `tools/vi-compiler/src/codegen.rs` | MODIFY — emit_child, if/for arms |
| `tools/vi-compiler/tests/codegen_tests.rs` | MODIFY — add if + for test cases |

---

## Implementation Steps

**Day 1 — ListView:**
1. Tạo `list_view.rs`: scaffold struct + builder + ViNode impl
2. Implement paint: visible range + clip + background highlight + text
3. Implement event: Scroll clamped + press/touch item select
4. Dirty subscription: items + selected signals
5. Register trong `node_widgets.rs` + `lib.rs`
6. `cargo check` main workspace

**Day 2 — AST + Parser:**
1. `ast.rs`: thêm `Child` enum, update Component/Element children type
2. `token.rs`: thêm KwIf/KwFor/KwIn
3. `lexer.rs`: thêm keyword entries
4. `parser.rs`: thêm parse_child + parse_if_child + parse_for_child
5. Fix compile errors từ Child type change
6. `cargo check` vi-compiler workspace

**Day 3 — Codegen + Tests:**
1. `codegen.rs`: thêm emit_child + if/for arms + desugar_prop_ref helper
2. `codegen_tests.rs`: test if + for cases → check generated Rust
3. `cargo test` vi-compiler
4. `cargo check` main workspace (ListView + existing widgets)

---

## Todo

- [ ] Tạo list_view.rs (struct, builder, paint, events, dirty subs)
- [ ] Register ListView trong node_widgets.rs + lib.rs
- [ ] ast.rs: Child enum + update children fields
- [ ] token.rs: KwIf, KwFor, KwIn
- [ ] lexer.rs: keyword entries
- [ ] parser.rs: parse_child + parse_if/for
- [ ] codegen.rs: emit_child + if/for + desugar_prop_ref
- [ ] codegen_tests: if + for test cases
- [ ] cargo check main workspace
- [ ] cargo test vi-compiler

---

## Success Criteria

- `ListView::new(items_signal)` với 10 items renders + scroll down/up
- Click item triggers on_select callback với đúng index
- `signal.update(|v| v.push("new".into()))` → ListView tự render lại
- `.vi` file với `if self.show { Button {} }` → vi-compiler ra Rust với `if *self.show.get()`
- `.vi` file với `for item in self.items { Label {} }` → vi-compiler ra Rust với `for ... in ... .iter()`
- Existing 36 codegen tests vẫn pass

---

## Risk

**Child enum refactor**: đổi `children: Vec<Element>` → `Vec<Child>` sẽ break nhiều match arm trong parser + codegen. Estimate 1-2h fix cascading errors. Kiểm tra tất cả `for child in &e.children` loops.

**desugar_prop_ref**: simple string replace `self.X` → `*self.X.get()` có thể false-positive trên string literals (e.g. `"self.value"`). G1 acceptable — P10 sẽ fix với proper AST-level Expr resolution.

**ListView scroll on touch**: `TouchMove` thường fire nhiều events, delta rất nhỏ. Accumulate delta trước khi scroll để tránh jitter.

---

## Evidence (Completed 2026-06-08)

**Verification:** `cargo check -p viui && cargo test -p vi-compiler`

**Deliverables verified:**
1. ✅ `libs/viui/src/node_widgets/list_view.rs` — ListView struct + builder pattern + paint (visible range clipping) + scroll events + selection + dirty propagation
2. ✅ `libs/viui/src/node_widgets.rs` — ListView registered (pub mod list_view; pub use list_view::ListView;)
3. ✅ `tools/vi-compiler/src/ast.rs` — Child enum added (Element | If | For variants)
4. ✅ `tools/vi-compiler/src/parser.rs` — parse_child + parse_if_child + parse_for_child implemented
5. ✅ `tools/vi-compiler/src/codegen.rs` — emit_child dispatcher + if/for desugaring + desugar_prop_refs helper
6. ✅ `tools/vi-compiler/tests/codegen_tests.rs` — 39/39 tests pass (existing + new if/for cases)

**Bugs fixed during reviewer verify:**
- Bug #1: `desugar_prop_refs` was emitting `*self.X.get()` but `build()` is a static fn → fixed to `*X.get()`
- Bug #2: `typed_expr_to_rust` for `bool` type emitted `Signal::new(String::from(true))` → fixed to `Signal::new(true)`

**Build status:** cargo check -p viui clean, cargo check -p vi-compiler clean
