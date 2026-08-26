# Phase 07 — Layout Engine v2 — FlexBox

**Status:** Planned  
**Stage:** G2  
**Priority:** High  
**Estimate:** 3-4 ngày  
**Depends on:** G1 complete (P01-P06)

---

## Context

Current layout: `Column` = vertical stack, `Row` = horizontal stack. Fixed. No:
- `min_width` / `max_width` constraints
- `weight` / `flex_grow` (fill remaining space)
- `align_items` (center/end/stretch)
- `justify_content` (space-between/around/evenly)
- Wrapping rows

G2 apps (desktop, server dashboard) need real flex layout.

---

## Architecture

### New `LayoutConstraint` passed to `ViNode::layout()`

```rust
// Current:
fn layout(&self, available_width: f32, available_height: f32) -> LayoutView;

// v2:
fn layout(&self, cx: LayoutCx) -> LayoutView;

pub struct LayoutCx {
    pub available_width:  f32,
    pub available_height: f32,
    pub min_width:        f32,
    pub min_height:       f32,
}
```

Breaking change — all ViNode implementations need updating. Plan accordingly.

### FlexBox node

```rust
// libs/viui/src/layout_flex.rs

pub struct FlexBox {
    children:      Vec<FlexChild>,
    direction:     FlexDirection,  // Row | Column
    wrap:          bool,
    justify:       Justify,        // Start | End | Center | SpaceBetween | SpaceAround
    align_items:   AlignItems,     // Start | End | Center | Stretch
    gap:           f32,
}

pub struct FlexChild {
    node:      Box<dyn ViNode>,
    flex_grow: f32,   // 0 = fixed, 1 = grow to fill
    min_size:  f32,
    max_size:  f32,
}
```

### Layout algorithm (simplified CSS flex)

1. Pass 1: measure all children with `flex_grow=0` → sum fixed widths
2. Remaining space = available_width - fixed_sum - gaps
3. Pass 2: distribute remaining to `flex_grow > 0` children proportionally
4. Pass 3: place children at computed positions
5. For wrap: when row fills → start new row

### Backward compat

`Column::new(children)` → internally creates `FlexBox { direction: Column, children: fixed_size }`.  
API stays same. Add `Column::weighted(vec![(child, weight)])` for grow support.

---

## Key files

```
libs/viui/src/layout_flex.rs    CREATE
libs/viui/src/node.rs           MODIFY — LayoutCx type
libs/viui/src/node_widgets/*.rs MODIFY — update layout() signature
```

---

## Migration strategy

- Add `LayoutCx` with backward-compat constructor: `LayoutCx::simple(w, h)` 
- Phase: update `layout()` signature with old args → LayoutCx adapter
- Test: existing Column/Row layout output unchanged

---

## Success Criteria

- `FlexBox` with `flex_grow` distributes remaining space correctly
- `justify_content: SpaceBetween` places items with equal gaps
- Backward: `Column::new([a, b, c])` layout result identical to current
- `cargo check` clean
