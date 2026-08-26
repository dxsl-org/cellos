# Phase 02 — Incremental Layout Gate

**Plan**: [plan.md](plan.md)
**Status**: Planned
**Depends on**: Phase 01 (DirtyRegion + rewired tick)
**Priority**: P2

---

## Overview

After Phase 01, `tick()` still runs `layout()` whenever `dirty_region.is_dirty()`
(even for pure signal changes). Phase 02 separates layout-dirty from paint-dirty
so that signal-only changes skip `root.layout()` entirely — O(m) paint instead
of O(n) layout + O(n) paint.

---

## Key Insights

- **When is layout needed?** Only when structural state changes: events consumed,
  `mark_dirty()` called, or first tick. Pure `Signal::set()` (text content changes,
  not text length) should NOT re-trigger layout.
- **When is re-subscribe needed?** Only after layout runs (bounds may change).
  If layout is skipped, handles from the previous layout are still correct.
- **Signal-only tick**: `dirty_region.is_dirty()` but `layout_dirty = false`.
  Use cached bounds (last layout). Re-subscribe not needed. Partial repaint only.
- **Layout change tick**: `layout_dirty = true`. Run layout → re-subscribe (update
  captured bounds) → `mark_all()` (full repaint, bounds shifted).
- **Caveat**: If `Label` text changes AND text length changes (→ label resizes),
  we need layout. But `Label::collect_dirty_handles` closure marks the *current*
  bounds which may be stale width. The rendered text will be clipped wrong.
  Mitigation: signal-only ticks use `mark_all()` if any widget opts in to
  "layout-affecting" signal. For P08 we accept this edge case and document it —
  the common case (counter increment showing same-length number) is correct.
  Full fix (layout dirty from size-changing signals) is G2 work.

---

## Requirements

### Functional
- `layout_dirty = true` only from: (a) events consumed, (b) `mark_dirty()`, (c) init
- Signal-only ticks: skip `root.layout()` and skip `collect_dirty_handles()`
- Layout tick: run layout → re-subscribe → `mark_all()` for full repaint
- Signal-only tick: render with `dirty_region.take()` (partial rect from subscription)

### Non-functional
- `app_runner.rs` ≤ 80 lines (P01 version should already be ~80; keep tight)
- `mark_dirty()` API unchanged (still forces full layout + full repaint)

---

## Architecture

### `libs/viui/src/app_runner.rs` — gate layout on `layout_dirty`

P01 left `tick()` always running layout when dirty_region is dirty. P02 splits:

```rust
pub fn tick(&mut self, events: &[Event]) -> bool {
    for e in events {
        if self.root.event(e) { self.layout_dirty = true; }
    }

    // Path A: structural change → layout + re-subscribe + full repaint
    if self.layout_dirty {
        self.layout_dirty = false;
        let (w, h) = self.renderer.size();
        self.root.layout(Constraints::root(Size::new(w as f32, h as f32)));
        self.dirty_handles = self.root.collect_dirty_handles(
            Rc::clone(&self.dirty_region)
        );
        self.dirty_region.borrow_mut().mark_all(w as f32, h as f32);
    }

    // Path B: signal-only change → partial repaint with cached layout
    let damage = self.dirty_region.borrow_mut().take();
    match damage {
        None => false,
        Some(rect) => {
            let root = &self.root;
            self.renderer.render(Some(rect), &mut |canvas| {
                root.paint(canvas);
            });
            true
        }
    }
}
```

Key differences from P01:
- Layout only when `layout_dirty = true` (not when signal-only dirty)
- After layout: always `mark_all()` so Path B always fires with full damage this tick
- `dirty_handles` only refreshed on layout (not on signal-only tick)
- `renderer.render` always gets `Some(rect)` — never `None` (first tick gives full-screen rect)

### `mark_dirty()` stays correct

```rust
pub fn mark_dirty(&mut self) {
    self.layout_dirty = true;  // triggers Path A on next tick
}
```

Path A sets `mark_all()` → Path B fires with full rect. ✓

---

## Related Code Files

**Modify:**
- `libs/viui/src/app_runner.rs` — only file changed in this phase

---

## Implementation Steps

1. Refactor `tick()` to the split A/B structure above
2. Verify `layout_dirty` starts `true` in `new()` (for first frame full layout)
3. `cargo check -p viui` — should pass immediately (only control flow change)
4. `cargo clippy -p viui -- -D warnings` — clean

---

## Todo List

- [ ] Refactor tick() — split layout path from paint path
- [ ] cargo check -p viui passes
- [ ] cargo clippy clean

---

## Success Criteria

- **Signal-only tick**: `root.layout()` NOT called; `root.paint(canvas)` called with
  `Some(partial_rect)` as damage
- **Event tick**: `root.layout()` IS called; `renderer.render(Some(full_screen), ...)` called
- `app_runner.rs` stays ≤ 80 lines

---

## Risk

- **Signal fires during layout**: if `on_click` → `signal.set()` → subscriber fires →
  marks `dirty_region` → layout also runs (layout_dirty=true) → mark_all overwrites
  dirty_region. Result: full repaint. Correct.
- **No events, no signals**: both paths skipped → return `false`. Correct (no frame).
- **Double mark_all**: Path A always calls mark_all; Path B always renders. First tick
  after any event = always a full repaint. Intended.
