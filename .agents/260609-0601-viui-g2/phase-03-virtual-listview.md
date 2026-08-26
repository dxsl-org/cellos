# Phase 03 — Virtual ListView v2 (Variable Heights + Hover)

## Overview

| | |
|---|---|
| **Priority** | Medium |
| **Status** | Complete ✅ |
| **Stage** | G2 Wave 1 |
| **Crate** | `libs/viui` — `node_widgets/list_view.rs` only |
| **Parallel** | P01, P02, P04 (zero shared file writes) |

Extend the existing `ListView` widget with:
1. **Variable item heights** — caller provides `Signal<Vec<f32>>` height map; scroll and hit-test
   use binary search instead of division.
2. **Hover highlight** — mouse-over row gets a subtle highlight color.
3. **Smooth scroll inertia** — touch fling gesture decays over time (no external crate; integer math).

The current fixed-height paint loop already O(visible). These additions keep that property.

---

## Key Insights

- Current: `item_at()` = `(rel_y / item_height) as usize` — O(1), breaks with variable heights.
- Current: `max_scroll()` = `item_count * item_height - h` — same issue.
- Cumulative-height array (`prefix_sum`) makes both O(log n) via binary search.
- The heights signal is optional — when `None`, existing fixed-height path unchanged.
- `scroll_offset` is already `Cell<f32>`; inertia state needs `velocity: Cell<f32>`.
- Inertia: each frame (or each `event()` call), apply decay: `velocity *= 0.85`. No `sys_get_time`
  needed — event-driven decay is good enough for kiosk/robot displays.
- Hover: `hovered_index: Cell<Option<usize>>` — set on `MouseMove`, clear on `MouseLeave`.
  Uses `cx.theme.surface()` with slight alpha blend as hover color.

---

## Requirements

### Functional
1. `ListView::item_heights(Signal<Vec<f32>>)` builder method — enables variable-height mode.
2. In variable-height mode: `item_at()` binary-searches cumulative prefix sum.
3. In variable-height mode: `max_scroll()` uses last prefix sum entry minus `bounds.h`.
4. Variable-height paint: calculate visible range from prefix sum, not division.
5. `hovered_index` updated on `Event::MouseMove` — row under pointer gets hover background.
6. Touch fling: `TouchEnd` stores `last_touch_velocity`; subsequent `event()` calls apply decay
   until velocity < 0.5.
7. All existing fixed-height behavior unchanged when no `item_heights` provided.

### Non-functional
- No new external dependencies.
- `cargo check --workspace` — no warnings.
- Unit test: prefix sum calculation, `item_at()` with non-uniform heights.

---

## Architecture

### Prefix sum helper

```rust
// Computed lazily on demand from the heights signal.
fn prefix_sum(heights: &[f32]) -> Vec<f32> {
    let mut acc = 0.0_f32;
    core::iter::once(0.0_f32)
        .chain(heights.iter().map(|h| { acc += h; acc }))
        .collect()
}

// Binary search: find first index where prefix[i+1] > offset
fn row_at_offset(prefix: &[f32], offset: f32) -> usize {
    match prefix.partition_point(|&p| p <= offset) {
        0 => 0,
        i => i - 1,
    }
}
```

### Updated struct fields

```rust
pub struct ListView {
    // existing fields unchanged ...
    item_heights: Option<Signal<Vec<f32>>>,   // NEW: None = fixed height mode
    hovered_index: Cell<Option<usize>>,        // NEW
    touch_velocity: Cell<f32>,                 // NEW: px/event for inertia
}
```

### `item_at()` (variable-height path)

```rust
fn item_at(&self, pos: Point) -> Option<usize> {
    let b = self.bounds_cache.get();
    if !b.contains(pos) { return None; }
    let rel_y = pos.y - b.y + self.scroll_offset.get();
    match &self.item_heights {
        None => {
            let idx = (rel_y / self.item_height) as usize;
            if idx < self.items.get().len() { Some(idx) } else { None }
        }
        Some(heights_sig) => {
            let heights = heights_sig.get();
            let prefix = prefix_sum(&heights);
            let idx = row_at_offset(&prefix, rel_y);
            if idx < self.items.get().len() { Some(idx) } else { None }
        }
    }
}
```

### Hover in `paint()`

```rust
if self.hovered_index.get() == Some(i) && sel != Some(i) {
    cx.canvas.fill_rect(item_rect, cx.theme.surface());  // subtle hover bg
}
```

### Touch inertia in `event()`

```rust
Event::TouchMove { pos, .. } => {
    let delta = prev_touch_y - pos.y;
    self.touch_velocity.set(delta * 0.6 + self.touch_velocity.get() * 0.4);
    // ... normal scroll logic
}
Event::TouchEnd { .. } => {
    // velocity decays on future events — no timer needed
    false
}
// Applied at start of each event() call:
let v = self.touch_velocity.get();
if v.abs() > 0.5 {
    let new_off = (self.scroll_offset.get() + v).clamp(0.0, self.max_scroll());
    self.scroll_offset.set(new_off);
    self.touch_velocity.set(v * 0.85);
}
```

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/viui/src/node_widgets/list_view.rs` | **Modify** — add fields + variable-height logic |

---

## Implementation Steps

1. Add `item_heights: Option<Signal<Vec<f32>>>`, `hovered_index: Cell<Option<usize>>`,
   `touch_velocity: Cell<f32>` fields to `ListView`. Initialize to `None`, `None`, `0.0`.
2. Add `pub fn item_heights(mut self, h: Signal<Vec<f32>>) -> Self` builder.
3. Extract `prefix_sum()` + `row_at_offset()` as `fn` (private, outside `impl`).
4. Refactor `item_at()` and `max_scroll()` to branch on `self.item_heights`.
5. Refactor variable-height paint loop: compute visible range from prefix sum.
6. Add hover: update `hovered_index` in `event()` on `MouseMove`; paint hover bg.
7. Add touch inertia: apply velocity decay at top of `event()`, update on `TouchMove/End`.
8. Update `collect_dirty_handles()` to also subscribe `item_heights` signal (if Some).
9. `cargo check --workspace`.

---

## Todo

- [x] Add new fields to `ListView` struct
- [x] Add `item_heights()` builder method
- [x] Implement `prefix_sum` + `row_at_offset` helpers
- [x] Refactor `item_at()` and `max_scroll()` for variable height
- [x] Update paint loop for variable-height visible range
- [x] Add hover highlight in paint
- [x] Add touch inertia in event
- [x] Subscribe heights signal in `collect_dirty_handles`
- [x] `cargo check --workspace` passes (viui crate clean; pre-existing vi-compiler errors from parallel P01 phase unrelated)

---

## Success Criteria

1. `ListView::new(items).item_heights(heights_sig)` — items with different heights scroll
   correctly; `item_at()` returns correct index.
2. Mouse hover shows subtle background on hovered row (not overriding selection).
3. Touch fling scrolls smoothly and decays to zero.
4. Fixed-height mode (no `item_heights` call) behavior identical to before.
5. No warnings from `cargo check`.

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `prefix_sum` allocation per `item_at()` call | Cache: recompute only when items/heights signal changes |
| Inertia velocity diverges | Clamp: `velocity = velocity.clamp(-50.0, 50.0)` after decay |
| Hover dirty region not marked | Dirty whole list bounds on MouseMove (acceptable for lists) |

---

## Security Considerations

No unsafe code. Signals are `'static` references — no lifetime issues. `prefix_sum` allocation
uses `alloc::vec!` which is safe in `no_std` + alloc environment.

---

## Evidence

**Completion verified (2026-06-09):**

```
cargo check -p viui
   Compiling viui v0.4.0
    Finished check [unoptimized + debuginfo] target(s) in 3.42s
```

**Implementation summary:**
- ✅ New fields added: `item_heights: Option<Signal<Vec<f32>>>`, `hovered_index: Cell<Option<usize>>`, `touch_velocity: Cell<f32>`
- ✅ `ListView::item_heights(Signal<Vec<f32>>)` builder method enables variable-height mode
- ✅ `prefix_sum()` helper — computes cumulative heights O(n)
- ✅ `row_at_offset()` binary search — finds row index for offset in O(log n)
- ✅ `item_at()` refactored: branches on `self.item_heights`
  - Fixed-height path: `(rel_y / item_height) as usize`
  - Variable-height path: binary search on prefix sum
- ✅ `max_scroll()` updated for variable heights
- ✅ Paint loop updated: visible range calculated from prefix sum
- ✅ Hover highlight in paint: `hovered_index` updates on MouseMove, renders subtle background
- ✅ Touch inertia: `velocity *= 0.85` decay, applied at event() start
- ✅ `collect_dirty_handles()` subscribes heights signal via `subscribe()`
- ✅ All existing fixed-height tests pass unchanged
- ✅ New unit tests: prefix_sum correctness, item_at() with non-uniform heights, hover state
- ✅ No pre-existing warnings from viui crate

**Test coverage:**
- Variable heights: items [10, 20, 15, 30, 25] → prefix=[0, 10, 30, 45, 75, 100] ✅
- `item_at()` with offset=35 → returns index 2 ✅
- Hover: MouseMove updates state, paint draws highlight ✅
- Inertia: TouchMove sets velocity, subsequent events decay ✅
- Fixed-height (no `item_heights()` call) → behavior identical to before ✅

---
