# Phase 01 — Signal→DirtyRect Wiring

**Plan**: [plan.md](plan.md)
**Status**: Planned
**Priority**: P1 — foundation; Phase 02 depends on this

---

## Overview

Wire `Signal<T>` subscriptions to `DirtyRect` accumulation so that when a widget's
signal fires, only that widget's screen bounds are queued for repaint — not the
whole frame. Then plumb the accumulated rect through `ViApp::tick()` to
`renderer.render(Some(damage), ...)`, finally activating the P07 `CpuExecutor`
damage-rect filtering that is currently dead code.

---

## Key Insights

- `DirtyRect` is already complete in `dirty.rs` — no changes to the struct.
- `Signal::subscribe(f)` accepts any `Fn() + 'static` — we subscribe closures
  that capture a `Rc<RefCell<DirtyRect>>` and the widget's current bounds `Rect`.
- Subscriptions capture a COPY of `bounds` at the time of `collect_dirty_handles()`
  (post-layout). Re-calling after every layout pass keeps captured bounds fresh.
  `Rect` is `Copy` and 16 bytes — cheap to copy into each closure.
- `ViNode` trait (in `libs/viui/`, NOT `libs/api/`) — adding a default method is
  backward-compatible; does NOT trigger Law 1.
- Column/Row don't have Signals themselves — they only recurse into children.

---

## Requirements

### Functional
- `DirtyRegion = Rc<RefCell<DirtyRect>>` type alias available from `viui::dirty`
- `ViNode::collect_dirty_handles(&mut self, region: DirtyRegion) -> Vec<SubscriptionHandle>` — default returns empty vec
- `Label` subscribes `self.text` → `region.borrow_mut().mark(self.bounds)`
- `Column` + `Row` recurse into children, flatmap their handles
- `ViApp` stores `dirty_region: DirtyRegion` and `dirty_handles: Vec<SubscriptionHandle>`
- After every layout, `ViApp` calls `collect_dirty_handles` to re-subscribe with current bounds
- `tick()` passes `dirty_region.borrow_mut().take()` to `renderer.render()` (replaces hard-coded `None`)
- First tick: `dirty_region` starts with `mark_all()` to force initial full frame

### Non-functional
- No unsafe code added
- Existing `ViApp::mark_dirty()` still works (calls `dirty_region.borrow_mut().mark_all(w, h)`)
- `cargo check -p viui` + `cargo clippy -p viui -- -D warnings` clean

---

## Architecture

### `libs/viui/src/dirty.rs` — add `DirtyRegion`

```rust
extern crate alloc;
use alloc::rc::Rc;
use core::cell::RefCell;

/// Shared handle to a `DirtyRect` accumulator — passed to widget subscriptions.
pub type DirtyRegion = Rc<RefCell<DirtyRect>>;
```

### `libs/viui/src/node.rs` — add `collect_dirty_handles`

```rust
extern crate alloc;
use alloc::vec::Vec;
use crate::dirty::DirtyRegion;
use crate::signal::SubscriptionHandle;

pub trait ViNode: 'static {
    // ... existing 4 methods unchanged ...

    /// Subscribe all Signal fields to mark `region` dirty on change.
    ///
    /// Called by `ViApp` after each layout pass with updated bounds.
    /// Old handles (from previous layout) must be dropped before calling again.
    /// Default: no-op (widgets with no Signals don't override).
    fn collect_dirty_handles(&mut self, _region: DirtyRegion) -> Vec<SubscriptionHandle> {
        Vec::new()
    }
}
```

### `libs/viui/src/node_widgets/label.rs` — implement `collect_dirty_handles`

```rust
use crate::dirty::DirtyRegion;
use crate::signal::SubscriptionHandle;
use alloc::vec::Vec;

impl ViNode for Label {
    // ... existing layout / bounds / paint / event unchanged ...

    fn collect_dirty_handles(&mut self, region: DirtyRegion) -> Vec<SubscriptionHandle> {
        let rect = self.bounds;  // Copy — captures current layout result
        let handle = self.text.subscribe(move || {
            region.borrow_mut().mark(rect);
        });
        alloc::vec![handle]
    }
}
```

**Note**: `rect` is captured by value (Copy) into the closure. If layout runs again
(P02 logic), `collect_dirty_handles` is called again — old handles are dropped,
new handles capture new bounds. Stale bounds only matter between layout and
the next re-subscribe call, which is safe because layout_dirty triggers mark_all.

### `libs/viui/src/node_widgets/column.rs` — implement `collect_dirty_handles`

```rust
use crate::dirty::DirtyRegion;
use crate::signal::SubscriptionHandle;
use alloc::vec::Vec;

impl ViNode for Column {
    // ... existing methods unchanged ...

    fn collect_dirty_handles(&mut self, region: DirtyRegion) -> Vec<SubscriptionHandle> {
        let mut handles = Vec::new();
        for child in &mut self.children {
            handles.extend(child.collect_dirty_handles(Rc::clone(&region)));
        }
        handles
    }
}
```

Need `use alloc::rc::Rc;` in column.rs (currently uses `alloc::boxed::Box` +
`alloc::vec::Vec` — add `Rc`).

### `libs/viui/src/node_widgets/row.rs` — same pattern as Column

Read `row.rs` first to confirm structure mirrors Column, then apply identical impl.

### `libs/viui/src/app_runner.rs` — rewire tick

```rust
use crate::dirty::{DirtyRect, DirtyRegion};
use crate::signal::SubscriptionHandle;
use alloc::rc::Rc;
use core::cell::RefCell;

pub struct ViApp {
    root:          Box<dyn ViNode>,
    renderer:      Box<dyn ViRenderer>,
    dirty_region:  DirtyRegion,
    dirty_handles: Vec<SubscriptionHandle>,
    layout_dirty:  bool,  // rename from dirty; Phase 02 will fully separate this
}

impl ViApp {
    pub fn new(root: Box<dyn ViNode>, renderer: Box<dyn ViRenderer>) -> Self {
        let dirty_region: DirtyRegion = Rc::new(RefCell::new(DirtyRect::new()));
        Self {
            root,
            renderer,
            dirty_region,
            dirty_handles: Vec::new(),
            layout_dirty: true,  // force layout+full repaint on first tick
        }
    }

    pub fn tick(&mut self, events: &[Event]) -> bool {
        for e in events {
            if self.root.event(e) { self.layout_dirty = true; }
        }

        // Always layout for now (Phase 02 will gate this on layout_dirty).
        // We always need to run layout before re-subscribing with correct bounds.
        if self.layout_dirty || self.dirty_region.borrow().is_dirty() {
            let (w, h) = self.renderer.size();
            self.root.layout(Constraints::root(Size::new(w as f32, h as f32)));
            // Re-subscribe with fresh bounds from this layout pass
            self.dirty_handles = self.root.collect_dirty_handles(
                Rc::clone(&self.dirty_region)
            );
            // Ensure at least a full repaint on layout change
            if self.layout_dirty {
                self.dirty_region.borrow_mut().mark_all(w as f32, h as f32);
            }
            self.layout_dirty = false;
        }

        let damage = self.dirty_region.borrow_mut().take();
        if damage.is_none() { return false; }

        let root = &self.root;
        self.renderer.render(damage, &mut |canvas| {
            root.paint(canvas);
        });
        true
    }

    /// Force a full repaint on the next tick.
    pub fn mark_dirty(&mut self) {
        // Get renderer size for mark_all — need to store w/h or use a flag.
        // For now: set layout_dirty so tick() calls mark_all with correct size.
        self.layout_dirty = true;
    }
}
```

**Note on `mark_dirty()`**: Post-P01 it sets `layout_dirty = true` which triggers
`mark_all()` inside tick. This preserves the existing public API contract.

---

## Related Code Files

**Modify:**
- `libs/viui/src/dirty.rs` — add `DirtyRegion` type alias
- `libs/viui/src/node.rs` — add `collect_dirty_handles` default method
- `libs/viui/src/node_widgets/label.rs` — implement `collect_dirty_handles`
- `libs/viui/src/node_widgets/column.rs` — implement `collect_dirty_handles`
- `libs/viui/src/node_widgets/row.rs` — implement `collect_dirty_handles`
- `libs/viui/src/app_runner.rs` — rewire tick + add dirty_region/dirty_handles fields

**Read first:**
- `libs/viui/src/node_widgets/row.rs` — confirm structure mirrors Column

---

## Implementation Steps

1. Read `libs/viui/src/node_widgets/row.rs` to confirm structure
2. `dirty.rs`: add `DirtyRegion` type alias + imports (Rc, RefCell, alloc)
3. `node.rs`: add `collect_dirty_handles` default method + imports
4. `node_widgets/label.rs`: implement `collect_dirty_handles`
5. `node_widgets/column.rs`: implement `collect_dirty_handles`
6. `node_widgets/row.rs`: implement `collect_dirty_handles`
7. `app_runner.rs`: restructure `ViApp` fields + rewrite `tick()`
8. `cargo check -p viui` — fix type errors
9. `cargo clippy -p viui -- -D warnings` — fix warnings

---

## Todo List

- [ ] Read row.rs
- [ ] dirty.rs: add DirtyRegion alias
- [ ] node.rs: add collect_dirty_handles default
- [ ] label.rs: impl collect_dirty_handles
- [ ] column.rs: impl collect_dirty_handles
- [ ] row.rs: impl collect_dirty_handles
- [ ] app_runner.rs: rewire tick()
- [ ] cargo check -p viui passes
- [ ] cargo clippy -p viui clean

---

## Success Criteria

- `Label::collect_dirty_handles()` subscribes `self.text` with a closure that
  captures current `self.bounds` and marks `dirty_region`
- `Column::collect_dirty_handles()` recurses into all children
- `ViApp::tick()` passes `Some(rect)` (not `None`) to `renderer.render()` when
  only a signal change occurred (no events)
- `cargo check -p viui` clean

---

## Risk Assessment

- **Stale bounds in closure**: if text length changes → label resizes → old closure
  has wrong bounds. Mitigated: P02 ensures layout runs (sets layout_dirty) when
  any event happens; layout always triggers re-subscribe with fresh bounds.
  Text-only changes (no resize) are the common case and work correctly.
- **Rc cycles**: `dirty_region` is stored in `ViApp` AND in closures held by
  `dirty_handles` (also in `ViApp`). When `ViApp` is dropped, both are dropped
  together. No cycle because `dirty_handles` don't reference `ViApp`.
- **Button not wired**: Button has no Signals (label is a plain `String`, not
  `Signal<String>`). Its `on_click` updates external Signals. After click:
  `event()` returns `true` → `layout_dirty = true` → full repaint. Correct.
