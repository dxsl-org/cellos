# Phase 02 — Column + Row + vstack!/hstack! + ViApp Runner

**Plan**: [plan.md](plan.md)  
**Depends on**: Phase 01 (ViNode, Label, Button) + P01 (ViRenderer)  
**Status**: Planned  
**Estimated**: 1–2 hours

---

## Context

Phase 01 cho leaf widgets. Phase này thêm:
1. `Column` + `Row` — layout containers chứa `Vec<Box<dyn ViNode>>`
2. `vstack!` / `hstack!` macros
3. `ViApp` — minimal tick-based app runner (events → layout → render)

Sau phase này, có thể viết counter demo:
```rust
let count = Signal::new(0i32);
let label = Label::new(count.map(|n| format!("Count: {n}")));
let btn_count = count.clone();
let button = Button::new("Increment", move || btn_count.update(|n| *n += 1));
let root = vstack!(label, button);
let mut app = ViApp::new(Box::new(root), renderer);
loop { app.tick(&read_events()); }
```

---

## Architecture

### `node_widgets/column.rs` — Column (vstack)

```rust
use alloc::{boxed::Box, vec::Vec};
use crate::canvas::ViCanvas;
use crate::event::Event;
use crate::layout::{Constraints, Point, Rect, Size};
use crate::node::ViNode;

pub struct Column {
    pub children: Vec<Box<dyn ViNode>>,
    pub spacing:  f32,
    pub padding:  f32,
    bounds:       Rect,
}

impl Column {
    pub fn new(children: Vec<Box<dyn ViNode>>) -> Self {
        Self { children, spacing: 4.0, padding: 0.0, bounds: Rect::ZERO }
    }

    pub fn with_spacing(mut self, s: f32) -> Self { self.spacing = s; self }
    pub fn with_padding(mut self, p: f32) -> Self { self.padding = p; self }
}

impl ViNode for Column {
    fn layout(&mut self, constraints: Constraints) -> Size {
        let pad = self.padding;
        let sp  = self.spacing;
        let mut y    = constraints.origin.y + pad;
        let x        = constraints.origin.x + pad;
        let inner_w  = (constraints.max.w - 2.0 * pad).max(0.0);
        let mut used_h = pad;

        for child in &mut self.children {
            let child_max = Size {
                w: inner_w,
                h: (constraints.max.h - used_h - pad).max(0.0),
            };
            let child_size = child.layout(Constraints::new(Point::new(x, y), child_max));
            y      += child_size.h + sp;
            used_h += child_size.h + sp;
        }

        // Subtract final spacing overshoot; add bottom padding
        if !self.children.is_empty() {
            used_h -= sp;
        }
        used_h += pad;

        let size = constraints.constrain(Size { w: constraints.max.w, h: used_h });
        self.bounds = Rect::from_origin_size(constraints.origin, size);
        size
    }

    fn bounds(&self) -> Rect { self.bounds }

    fn paint(&self, canvas: &mut dyn ViCanvas) {
        for child in &self.children { child.paint(canvas); }
    }

    fn event(&mut self, event: &Event) -> bool {
        // Bottom-up routing: deepest (last) child first, bubbles up
        for child in self.children.iter_mut().rev() {
            if child.event(event) { return true; }
        }
        false
    }
}
```

### `node_widgets/row.rs` — Row (hstack)

Same structure as Column but stacks horizontally. `x` advances instead of `y`.

### `vstack!` / `hstack!` macros

```rust
// In lib.rs (or macros.rs) — #[macro_export] so they're at crate root
#[macro_export]
macro_rules! vstack {
    ($($child:expr),* $(,)?) => {
        $crate::node_widgets::column::Column::new(
            alloc::vec![$( alloc::boxed::Box::new($child) as alloc::boxed::Box<dyn $crate::node::ViNode> ),*]
        )
    }
}

#[macro_export]
macro_rules! hstack {
    ($($child:expr),* $(,)?) => {
        $crate::node_widgets::row::Row::new(
            alloc::vec![$( alloc::boxed::Box::new($child) as alloc::boxed::Box<dyn $crate::node::ViNode> ),*]
        )
    }
}
```

### `app_runner.rs` — ViApp

```rust
use alloc::boxed::Box;
use crate::event::Event;
use crate::layout::{Constraints, Size};
use crate::node::ViNode;
use crate::renderer::ViRenderer;

/// Minimal tick-based app runner for ViUI v2.
///
/// Does not own the event source — call `tick()` from your cell's main loop.
///
/// # Repaint strategy (P02)
///
/// Full repaint on any event. Fine-grained dirty-rect optimization is P04 work
/// (requires stable widget bounds wired into Signal subscriptions after layout).
pub struct ViApp {
    root:     Box<dyn ViNode>,
    renderer: Box<dyn ViRenderer>,
    dirty:    bool,
}

impl ViApp {
    /// Create a new app. The first tick will always render a full frame.
    pub fn new(root: Box<dyn ViNode>, renderer: Box<dyn ViRenderer>) -> Self {
        Self { root, renderer, dirty: true }
    }

    /// Process a slice of input events, then render if dirty.
    ///
    /// Returns `true` if a frame was rendered this tick.
    pub fn tick(&mut self, events: &[Event]) -> bool {
        for e in events {
            if self.root.event(e) {
                self.dirty = true;
            }
        }

        if !self.dirty { return false; }
        self.dirty = false;

        let (w, h) = self.renderer.size();
        self.root.layout(Constraints::root(Size::new(w as f32, h as f32)));

        // Paint: borrows root immutably + renderer mutably — different fields, safe with NLL.
        let root = &self.root;
        self.renderer.render(None, &mut |canvas| {
            root.paint(canvas);
        });

        true
    }

    /// Force a repaint on the next tick.
    pub fn mark_dirty(&mut self) { self.dirty = true; }
}
```

---

## Borrow-checker note on `tick()`

`self.root.paint()` borrows `root` immutably.  
`self.renderer.render()` borrows `renderer` mutably.  
These are **different fields** → Rust NLL field-borrow splitting allows this.  
The `let root = &self.root;` rebind before the `self.renderer.render()` call makes the split explicit.

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/viui/src/node_widgets/column.rs` | **CREATE** |
| `libs/viui/src/node_widgets/row.rs` | **CREATE** |
| `libs/viui/src/node_widgets.rs` | **MODIFY** — add `pub mod column; pub mod row;` |
| `libs/viui/src/app_runner.rs` | **CREATE** |
| `libs/viui/src/lib.rs` | **MODIFY** — add `pub mod app_runner;` + macros |

---

## Implementation Steps

1. Create `libs/viui/src/node_widgets/column.rs`
2. Create `libs/viui/src/node_widgets/row.rs`
3. Update `libs/viui/src/node_widgets.rs` — add `pub mod column; pub mod row;`
4. Create `libs/viui/src/app_runner.rs`
5. Add `vstack!` + `hstack!` macros to `lib.rs` (or `macros.rs`)
6. Add `pub mod app_runner;` to `lib.rs`
7. `cargo check -p viui` — zero warnings

---

## Success Criteria

- [ ] `cargo check -p viui` clean
- [ ] `vstack!(label, button)` compiles to `Column`
- [ ] `Column::layout()` assigns non-zero bounds to each child
- [ ] `ViApp::new(Box::new(root), Box::new(renderer)).tick(&[])` compiles
- [ ] Borrow checker passes `tick()` — NLL field split works

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| NLL field-borrow in `tick()` | Tested pattern — different fields. If rejected, use `mem::replace` workaround |
| `Box<dyn ViNode>` coercion in macros | Explicit `as Box<dyn ViNode>` cast in macro body |
| Column height overflow | `max(0.0)` clamp on child_max.h |
| vstack!/hstack! alloc in no_std | `extern crate alloc; use alloc::vec;` already at crate root |
