# Phase 08 — Virtual ListView + Performance

**Status:** Planned  
**Stage:** G2  
**Priority:** Medium  
**Estimate:** 2 ngày  
**Depends on:** Phase 01 (ListView baseline)

---

## Context

Phase 01 ListView uses non-virtual render: paints ALL items, relies on clip. O(n) paint per frame.
For n ≤ 200: acceptable (G1). For n = 1000+ (server log, file browser): 50ms+ paint → jank.

---

## Virtual Render

Only paint items in visible range:

```rust
// Current paint:
for i in 0..items.len() { paint_item(i) }  // O(n)

// Virtual paint:
let first = (scroll_offset / item_height).floor() as usize;
let last  = ((scroll_offset + bounds.height) / item_height).ceil() as usize + 1;
for i in first..last.min(items.len()) { paint_item(i) }  // O(visible)
```

This is already planned in Phase 01 as the `first..last` range computation — just need to actually skip painting items outside range rather than relying on clip.

---

## Heterogeneous item heights

G1: fixed item_height. G2: variable heights via `item_height_fn: Box<dyn Fn(usize) -> f32>`.

```rust
pub enum ItemHeight {
    Fixed(f32),
    Variable(Box<dyn Fn(usize) -> f32>),
}
```

For variable heights: precompute cumulative offsets array → O(1) lookup with binary search.

---

## Typed item rendering

G1: `Signal<Vec<String>>` (strings only). G2: generic `Signal<Vec<T>>` with `item_renderer: Box<dyn Fn(&T, Rect, &mut RenderCtx<'_>)>`:

```rust
pub struct ListView<T> {
    items:         Signal<Vec<T>>,
    item_renderer: Box<dyn Fn(&T, Rect, &mut RenderCtx<'_>)>,
    item_height:   ItemHeight,
    // ...
}
```

Backward compat: `ListView::new(Signal<Vec<String>>)` stays as convenience constructor with default string renderer.

---

## Success Criteria

- ListView with 10,000 items renders in < 2ms per frame
- Scroll is smooth (no O(n) paint per frame)
- Fixed item height: identical visual output to Phase 01 baseline
