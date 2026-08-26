# Phase 04 — ListView + DSL Completeness

**Status:** Planned  
**Priority:** Medium — robot UI navigation + dynamic content  
**Estimate:** 2-3 ngày  
**Depends on:** Phase 01 (RenderCtx), Phase 02 (node_widgets pattern)

---

## Context Links

- [`tools/vi-compiler/src/codegen.rs`](../../../tools/vi-compiler/src/codegen.rs) — codegen chưa xử lý if/for
- [`tools/vi-compiler/src/parser.rs`](../../../tools/vi-compiler/src/parser.rs) — if/for đã parse được
- [`libs/viui/src/node_widgets/`](../../../libs/viui/src/node_widgets/) — pattern cho ListView

---

## Overview

Hai deliverables độc lập trong phase này:

1. **ListView**: widget hiển thị danh sách items từ `Signal<Vec<T>>`, cần thiết cho:
   - Robot: log events, sensor history, task queue
   - Kiosk: menu items, option lists

2. **DSL if/for codegen**: conditional + loop trong .vi files compile ra Rust hợp lệ

---

## Part A — ListView Widget

### Requirements

- `Signal<Vec<String>>` items (generic string list đủ cho G1)
- Item height cố định (configurable, default 32px)
- Scrollable khi content > height
- Selected item highlighting via `Signal<Option<usize>>`
- on_select callback: `Box<dyn Fn(usize)>`

### Architecture

```rust
// libs/viui/src/node_widgets/list_view.rs

pub struct ListView {
    items:       Signal<Vec<String>>,
    selected:    Signal<Option<usize>>,
    on_select:   Option<Box<dyn Fn(usize)>>,
    item_height: f32,
    scroll_offset: Cell<f32>,
    bounds_cache:  Cell<Rect>,
    _subs:       Vec<SubscriptionHandle>,
}

impl ListView {
    pub fn new(items: Signal<Vec<String>>) -> Self { ... }
    pub fn item_height(mut self, h: f32) -> Self { ... }
    pub fn on_select(mut self, f: impl Fn(usize) + 'static) -> Self { ... }
    pub fn selected(mut self, sel: Signal<Option<usize>>) -> Self { ... }
}
```

**Paint logic:**
1. Compute visible range: `first = (scroll_offset / item_height) as usize`
2. For each visible item: paint background (selected = highlight color, else transparent), paint text
3. Clip to ListView bounds (via canvas.clip_push/pop)

**Event handling:**
- `Scroll { delta_y }` → update scroll_offset (clamp to content height)
- `MousePress { pos }` → compute clicked item index → set selected + call on_select
- `TouchBegin` → same as MousePress
- `TouchMove` with large delta → scroll (pan gesture)

**Non-virtual render** (G1 simplification): render all items, rely on clip to hide off-screen.
For G2, add virtual rendering (only items in visible range).

### Data binding pattern

```rust
// Usage:
let log_items: Signal<Vec<String>> = Signal::new(Vec::new());
let list = ListView::new(log_items.clone());

// Add item from sensor callback:
log_items.update(|v| v.push(format!("Sensor A: {:.2}V", reading)));
```

---

## Part B — DSL if/for Codegen

### Current state (parser.rs)

Parser recognizes:
- `if condition { ... }` → `Element::If { cond: Expr, body: Vec<Element> }`
- `for item in items { ... }` → `Element::For { var: String, iter: Expr, body: Vec<Element> }`

But `codegen.rs` skips these — falls through to empty match arm.

### if codegen

**Target output:**

```rust
// .vi source:
if self.show_panel { Panel {} }

// Generated Rust:
fn paint(&self, cx: &mut RenderCtx<'_>) {
    if *self.show_panel.get() {
        // paint Panel
        self.panel_child.paint(cx);
    }
}
```

For layout: `if` children participate conditionally in layout pass.

Implementation in `codegen.rs`:

```rust
Element::If { cond, body } => {
    // Emit: if *self.{signal}.get() { ... }
    let cond_rust = emit_expr(cond, component);
    writeln!(out, "if {cond_rust} {{")?;
    for child in body {
        emit_element(out, child, component)?;
    }
    writeln!(out, "}}")?;
}
```

Signal properties used in `if` conditions need `*self.{prop}.get()` desugaring.
`emit_expr()` phải handle property references → `*self.{name}.get()`.

### for codegen

**Target output:**

```rust
// .vi source:
for item in self.items { Text { text: item } }

// Generated Rust:
fn paint(&self, cx: &mut RenderCtx<'_>) {
    let __y = pos.y;
    for (i, item) in self.items.get().iter().enumerate() {
        // paint Text with item
        let pos = Point { x: pos.x, y: __y + i as f32 * LINE_HEIGHT };
        draw_text_scaled(pos, item, ...);
    }
}
```

Layout: `for` loops require dynamic layout. Emit a Column-like stacking loop in `layout()`.

This is the most complex codegen change. Strategy for G1:
- **Vertical-only** `for` loop: items stack vertically with fixed item height
- `items` must be a `Signal<Vec<String>>` property (typed constraint)
- Generated struct holds the items signal as a field

```rust
Element::For { var, iter, body } => {
    // Emit loop over iter.get().iter()
    writeln!(out, "for (_{var}_idx, {var}) in {iter}.get().iter().enumerate() {{")?;
    // body elements use {var} as a local binding
    emit_element_body(out, body, component, Some(var))?;
    writeln!(out, "}}")?;
}
```

### Property binding completeness

Currently: property defaults are raw expressions, not reactive bindings.

```vi
// Currently works:
property text: "Hello"

// Currently broken (codegen emits literal, not reactive):
property color: parent.theme_color
```

Fix: in `eval.rs`, detect when a default expression is a property reference (identifier or `self.X`),
emit a `Signal::map()` binding instead of a static init.

This is complex — **defer to Phase 04b** if time is short. The if/for codegen is higher value.

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/viui/src/node_widgets/list_view.rs` | CREATE |
| `libs/viui/src/node_widgets.rs` | MODIFY — add ListView |
| `libs/viui/src/lib.rs` | MODIFY — re-export ListView |
| `tools/vi-compiler/src/codegen.rs` | MODIFY — if/for codegen |
| `tools/vi-compiler/src/eval.rs` | MODIFY — property ref → Signal::map |
| `tools/vi-compiler/tests/codegen_tests.rs` | MODIFY — add if/for test cases |

---

## Implementation Steps

**ListView (2 days):**
1. Tạo `list_view.rs` với layout + paint + event (scroll + select)
2. Register trong `node_widgets.rs` + `lib.rs`
3. Test: `Signal<Vec<String>>` với 10 items, scroll down, click selects

**DSL if codegen (1 day):**
1. `codegen.rs`: implement `Element::If` match arm → emit conditional paint/layout
2. `eval.rs`: `emit_expr()` handles `PropertyRef("self.X")` → `*self.X.get()`
3. Add test: `.vi` với `if show { Button {} }` → check generated Rust has `if`

**DSL for codegen (0.5-1 day):**
1. `codegen.rs`: implement `Element::For` match arm → emit loop
2. Add test: `.vi` với `for item in items { Text {} }` → check generated loop

---

## Todo

- [ ] Tạo list_view.rs (paint, event/scroll/select, Signal<Vec<String>>)
- [ ] Register ListView trong node_widgets.rs + lib.rs
- [ ] codegen.rs: Element::If → emit conditional Rust
- [ ] eval.rs: property reference → Signal::map desugaring  
- [ ] codegen.rs: Element::For → emit loop Rust
- [ ] Thêm codegen_tests: if + for cases
- [ ] cargo check (vi-compiler standalone workspace)
- [ ] Cargo check (main workspace với ListView)

---

## Success Criteria

- `ListView::new(items_signal)` với 5 items renders + scrolls
- Clicking item gọi `on_select(index)`
- `.vi` file với `if show { Button {} }` compile qua `vi-compiler` → valid Rust với `if *self.show.get()`
- `.vi` file với `for i in items { Text {} }` → valid Rust với `for ... in ... .get().iter()`
- Tất cả existing codegen_tests vẫn pass

---

## Risk

**DSL for loop layout**: codegen-generated layout cho `for` loop phức tạp hơn paint.
Layout cần biết số items tại compile time — không biết. Giải pháp: generated `layout()` call
`items.get().len()` và trả về tổng height = len * item_height. Conservative nhưng đúng.

**ListView virtual render**: Non-virtual render O(n) paint với n=1000 items sẽ chậm.
G1 giới hạn n ≤ 100 (robot log list không cần hơn). G2 thêm virtual render.
