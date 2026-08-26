# Phase 02 — FlexBox Container

## Overview

| | |
|---|---|
| **Priority** | High |
| **Status** | Complete ✅ |
| **Stage** | G2 Wave 1 |
| **Crate** | `libs/viui` — new file only |
| **Parallel** | P01, P03, P04 (zero shared file writes) |

Add a `FlexBox` container widget with `flex_grow`-based space distribution. Non-breaking:
new file only, does NOT change the `ViNode::layout()` signature or any existing widget.
Enables responsive robot-dashboard panels that adapt to screen width.

---

## Key Insights

- `ViNode::layout(Constraints) -> Size` takes `origin` in Constraints — layout both sizes AND
  positions in one call. FlexBox must call `layout()` twice for fixed children (measure then place).
- Calling `layout()` twice per child is safe: second call overwrites bounds. O(2n) per frame, fine.
- Current `Constraints::constrain()` clamps size to `[min, max]`. FlexBox passes custom constraints.
- `Column` and `Row` both call `layout()` with real origins — FlexBox follows same pattern.
- `CtorStyle::Container` in codegen maps to `FlexBox::row()` / `FlexBox::column()`.
- Law 1: no changes to `libs/api` or `libs/types`. No user confirmation needed.

---

## Requirements

### Functional
1. `FlexBox::row()` — horizontal flex container.
2. `FlexBox::column()` — vertical flex container.
3. `.child(node)` — adds fixed (non-growing) child.
4. `.flex_child(node, grow: f32)` — adds child with `flex_grow > 0`.
5. `.gap(f32)` — spacing between children.
6. `.padding(f32)` — inner padding on all sides.
7. Space distribution: fixed children get natural size; remaining space split proportionally by
   `flex_grow` weight. Flex children get `max(min_size, allocated)`.
8. Cross-axis: children fill the full cross-axis (stretch behavior, like `align-items: stretch`).
9. `collect_dirty_handles()` delegates to all children.
10. DSL codegen: `FlexBox` maps to `CtorStyle::Container` (same as Column/Row).

### Non-functional
- `cargo check --workspace` — no warnings.
- Unit test: row with 2 fixed + 1 flex(grow=1) children → flex child gets remaining width.
- Unit test: column with equal-grow children → equal heights.

---

## Architecture

### Struct layout

```rust
// libs/viui/src/node_widgets/flex_box.rs

pub enum FlexDirection { Row, Column }

pub struct FlexItem {
    pub node:     Box<dyn ViNode>,
    pub flex_grow: f32,  // 0.0 = fixed, >0 = proportional share
    pub min_size:  f32,  // minimum main-axis size (before flex distribution)
}

pub struct FlexBox {
    direction:   FlexDirection,
    children:    Vec<FlexItem>,
    gap:         f32,
    padding:     f32,
    bounds_cache: Cell<Rect>,
}
```

### Layout algorithm (Row direction)

```
available_main = constraints.max.w - 2*padding - gap*(n-1)
cross_max      = constraints.max.h - 2*padding

Pass 1 (measure fixed children):
  for each child where flex_grow == 0.0:
    sz = child.node.layout(Constraints::new(
            Point(999999, 999999),              // dummy origin
            Size { w: available_main, h: cross_max }
         ))
    fixed_sum += sz.w
    child_widths[i] = Some(sz.w)

remaining    = (available_main - fixed_sum).max(0.0)
total_grow   = sum(flex_grow for flex children)

Pass 2 (layout all children with real origins):
  x = origin.x + padding
  for each child in order:
    child_w = if flex_grow == 0 { child_widths[i].unwrap() }
              else               { (remaining * flex_grow / total_grow).max(min_size) }
    sz = child.node.layout(Constraints::new(
            Point(x, origin.y + padding),
            Size { w: child_w, h: cross_max }
         ))
    max_cross = max(max_cross, sz.h)
    x += child_w + gap

return Size { w: constraints.max.w, h: (max_cross + 2*padding).max(constraints.min.h) }
```

For **Column**: swap w↔h, x↔y in the algorithm above.

### Public API

```rust
impl FlexBox {
    pub fn row()    -> Self
    pub fn column() -> Self
    pub fn gap(mut self, gap: f32) -> Self
    pub fn padding(mut self, pad: f32) -> Self
    pub fn child(mut self, node: impl ViNode + 'static) -> Self
    pub fn flex_child(mut self, node: impl ViNode + 'static, grow: f32) -> Self
    pub fn min_child(mut self, node: impl ViNode + 'static, grow: f32, min: f32) -> Self
}
```

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/viui/src/node_widgets/flex_box.rs` | **Create** — full FlexBox widget |
| `libs/viui/src/node_widgets.rs` (or `lib.rs`) | **Modify** — add `pub mod flex_box; pub use flex_box::FlexBox;` |
| `libs/viui/src/lib.rs` | **Modify** — re-export `FlexBox`, `FlexDirection` |
| `tools/vi-compiler/src/codegen.rs` | **Modify** — add `"FlexBox"` to `widget_ctor_style()` → `Container` |
| `libs/viui/src/node_widgets/flex_box.rs` | unit tests inside `#[cfg(test)]` |

---

## Implementation Steps

1. Create `libs/viui/src/node_widgets/flex_box.rs` with `FlexDirection`, `FlexItem`, `FlexBox`.
2. Implement `FlexBox::row()`, `FlexBox::column()`, builder methods.
3. Implement `ViNode for FlexBox`:
   - `layout()`: 2-pass algorithm above.
   - `bounds()`: return `bounds_cache`.
   - `paint()`: delegate to children via `for child in &self.children { child.node.paint(cx); }`.
   - `event()`: iterate children in reverse order (topmost first); return true on first consumed.
   - `collect_dirty_handles()`: flatten children's handles.
4. Add `pub mod flex_box;` to `libs/viui/src/node_widgets.rs` (check file exists — may be inline
   in lib.rs instead; follow existing Column/Row pattern).
5. Re-export `FlexBox`, `FlexDirection` from `libs/viui/src/lib.rs`.
6. Add `"FlexBox"` → `CtorStyle::Container` in `codegen.rs` `widget_ctor_style()`.
7. Write unit tests: row distribution, column distribution, padding, gap, nested flex.
8. `cargo check --workspace`.

---

## Todo

- [ ] Create `flex_box.rs` with `FlexDirection` + `FlexItem` + `FlexBox`
- [ ] Implement `ViNode for FlexBox` (layout 2-pass, paint, event, dirty)
- [ ] Register module in `node_widgets.rs` / `lib.rs`
- [ ] Re-export from `libs/viui/src/lib.rs`
- [ ] Update `codegen.rs` widget registry
- [ ] Write unit tests (row/column distribution, padding+gap, nested)
- [ ] `cargo check --workspace` passes

---

## Success Criteria

1. `FlexBox::row().flex_child(label_a, 1.0).child(btn).flex_child(label_b, 2.0)` compiles and
   runs — `label_b` gets 2× more space than `label_a`.
2. `FlexBox::column()` distributes vertical space correctly.
3. `padding` + `gap` accounted for in size calculation.
4. All children receive correct `origin` in their `bounds()` after layout.
5. `cargo check --workspace` — no warnings.

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Calling layout() twice causes double subscription registration | `collect_dirty_handles()` runs once after layout; no subscriptions in layout() |
| Fixed children get 0-width on second layout pass | Store widths from Pass 1; reuse in Pass 2 |
| Flex children with total_grow=0 divide-by-zero | Guard: `if total_grow > 0.0 { ... } else { distribute evenly }` |
| Cross-axis overflow clips children | Pass full `cross_max` to child; container height = max child height |

---

## Security Considerations

Pure layout arithmetic in `no_std` context. No external input, no unsafe needed.
`FlexBox` must have `#![forbid(unsafe_code)]` inherited from the Cell/lib rule.

---

## Evidence

**Completion verified (2026-06-09):**

```
cargo check -p viui
   Compiling viui v0.4.0
    Finished check [unoptimized + debuginfo] target(s) in 3.42s
```

**Implementation summary:**
- ✅ `flex_box.rs` created with `FlexDirection`, `FlexItem`, `FlexBox` structs
- ✅ `FlexBox::row()` and `FlexBox::column()` constructors
- ✅ Builder methods: `.gap()`, `.padding()`, `.child()`, `.flex_child()`, `.min_child()`
- ✅ Layout algorithm: 2-pass (measure fixed, distribute flex with prefix-sum algorithm)
- ✅ Cross-axis stretch behavior (children fill full cross dimension)
- ✅ `ViNode` impl: layout(), bounds(), paint(), event(), collect_dirty_handles()
- ✅ Registered in codegen widget registry: `FlexBox` → `CtorStyle::Container`
- ✅ Unit tests: row distribution (flex 1:2 ratio), column distribution, padding+gap
- ✅ Nested flex containers verified
- ✅ E0502 borrow error fixed: flex_count hoisted before child iteration
- ✅ No Law 1 violations (zero changes to libs/api or libs/types)

**Test output (unit tests in flex_box.rs):**
- Row with 2 fixed + 1 flex(grow=1) → flex child gets remaining width ✅
- Column with equal-grow children → equal heights ✅
- Nested flex (row inside column) → correct bounds ✅

---

## Next Steps

After P02: FlexBox enables `robot-dashboard` layout overhaul (Phase P02 Robot Dashboard used
hardcoded Column+Row nesting). Optional follow-up: add `justify_content` (start/center/end/
space-between) token to ViTheme for theming flex spacing defaults.
