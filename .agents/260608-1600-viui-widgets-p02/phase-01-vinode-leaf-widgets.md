# Phase 01 — ViNode Trait + Label + Button

**Plan**: [plan.md](plan.md)  
**Depends on**: P01 Signal engine (signal.rs, dirty.rs, renderer.rs) ✅  
**Status**: Planned  
**Estimated**: 1–2 hours

---

## Context

P01 đã có `Signal<T>`, `DirtyRect`, `ViRenderer`. Phase này xây dựng widget layer:
1. `ViNode` trait — giao diện thống nhất cho tất cả v2 widgets
2. `Label` — text widget đọc từ `Signal<String>` (không rebuild trên mỗi frame)
3. `Button` — clickable widget với `on_click: Box<dyn Fn()>` callback

v1 `ViWidget` trait giữ nguyên — không xóa, không ảnh hưởng.

---

## Architecture

### `node.rs` — ViNode trait

```rust
// libs/viui/src/node.rs
use crate::canvas::ViCanvas;
use crate::event::Event;
use crate::layout::{Constraints, Rect, Size};

/// v2 widget trait — Reactive Signal Tree node.
///
/// Compared to v1 ViWidget:
/// - No Msg generic (callbacks are Box<dyn Fn()> in widgets directly)
/// - No WidgetStateStore (state lives in Signal fields)
/// - layout() → Size (not LayoutNode tree — containers recurse into children)
/// - paint() → direct canvas (no PaintCx wrapping)
/// - event() → bool (no EventStatus enum, no EventCx)
pub trait ViNode: 'static {
    /// Compute layout given available constraints; returns occupied size.
    ///
    /// Implementations MUST cache their final bounds for `bounds()` and `paint()`.
    fn layout(&mut self, constraints: Constraints) -> Size;

    /// Cached bounds from the last `layout()` call.
    fn bounds(&self) -> Rect;

    /// Paint this widget into `canvas`. Uses cached bounds from layout().
    fn paint(&self, canvas: &mut dyn ViCanvas);

    /// Handle an input event. Returns `true` if consumed (stops bubbling).
    fn event(&mut self, event: &Event) -> bool;
}
```

### `node_widgets/label.rs` — Label v2

```rust
use alloc::string::String;
use crate::canvas::{Color, TextStyle, ViCanvas};
use crate::event::Event;
use crate::layout::{Constraints, Point, Rect, Size};
use crate::node::ViNode;
use crate::signal::Signal;

const GLYPH_W: f32 = 8.0;
const GLYPH_H: f32 = 8.0;

pub struct Label {
    pub text:  Signal<String>,
    pub color: Color,
    bounds:    Rect,
}

impl Label {
    pub fn new(text: Signal<String>) -> Self {
        Self { text, color: Color::WHITE, bounds: Rect::ZERO }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
}

impl ViNode for Label {
    fn layout(&mut self, constraints: Constraints) -> Size {
        let chars = self.text.get().chars().count();
        let desired = Size { w: chars as f32 * GLYPH_W, h: GLYPH_H };
        let size = constraints.constrain(desired);
        self.bounds = Rect::from_origin_size(constraints.origin, size);
        size
    }

    fn bounds(&self) -> Rect { self.bounds }

    fn paint(&self, canvas: &mut dyn ViCanvas) {
        canvas.draw_text(
            Point::new(self.bounds.x, self.bounds.y),
            &self.text.get(),
            TextStyle { color: self.color, size_px: 0 },
        );
    }

    fn event(&mut self, _event: &Event) -> bool { false }
}
```

### `node_widgets/button.rs` — Button v2

```rust
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use crate::canvas::{Color, TextStyle, ViCanvas};
use crate::event::{Event, MouseButton};
use crate::layout::{Constraints, Point, Rect, Size};
use crate::node::ViNode;

const PAD: f32 = 6.0;
const GLYPH_W: f32 = 8.0;
const GLYPH_H: f32 = 8.0;

pub struct Button {
    pub label:    String,
    pub on_click: Box<dyn Fn()>,
    hovered:      bool,
    pressed:      bool,
    bounds:       Rect,
}

impl Button {
    pub fn new(label: impl Into<String>, on_click: impl Fn() + 'static) -> Self {
        Self {
            label:    label.into(),
            on_click: Box::new(on_click),
            hovered:  false,
            pressed:  false,
            bounds:   Rect::ZERO,
        }
    }
}

impl ViNode for Button {
    fn layout(&mut self, constraints: Constraints) -> Size {
        let chars = self.label.chars().count();
        let desired = Size {
            w: chars as f32 * GLYPH_W + PAD * 2.0,
            h: GLYPH_H + PAD * 2.0,
        };
        let size = constraints.constrain(desired);
        self.bounds = Rect::from_origin_size(constraints.origin, size);
        size
    }

    fn bounds(&self) -> Rect { self.bounds }

    fn paint(&self, canvas: &mut dyn ViCanvas) {
        let bg = if self.pressed      { Color::rgb(80, 80, 160) }
                 else if self.hovered { Color::rgb(70, 70, 130) }
                 else                 { Color::rgb(50, 50, 100) };
        canvas.fill_rect(self.bounds, bg);

        // Border
        let b = self.bounds;
        let border = Color::rgb(120, 120, 200);
        for (a, bb) in [
            (Point::new(b.x,       b.y),       Point::new(b.x + b.w, b.y)),
            (Point::new(b.x + b.w, b.y),       Point::new(b.x + b.w, b.y + b.h)),
            (Point::new(b.x + b.w, b.y + b.h), Point::new(b.x,       b.y + b.h)),
            (Point::new(b.x,       b.y + b.h), Point::new(b.x,       b.y)),
        ] {
            canvas.draw_line(a, bb, border);
        }

        canvas.draw_text(
            Point::new(b.x + PAD, b.y + PAD),
            &self.label,
            TextStyle { color: Color::WHITE, size_px: 0 },
        );
    }

    fn event(&mut self, event: &Event) -> bool {
        match event {
            Event::MouseMove { pos } => {
                self.hovered = self.bounds.contains(*pos);
                false
            }
            Event::MousePress { pos, button: MouseButton::Left } => {
                if self.bounds.contains(*pos) {
                    self.pressed = true;
                    true
                } else { false }
            }
            Event::MouseRelease { pos, button: MouseButton::Left } => {
                let was_pressed = self.pressed;
                self.pressed = false;
                if was_pressed && self.bounds.contains(*pos) {
                    (self.on_click)();
                    true
                } else { false }
            }
            _ => false,
        }
    }
}
```

---

## Related Code Files

| File | Action | Reuse from v1 |
|------|--------|---------------|
| `libs/viui/src/node.rs` | **CREATE** | — |
| `libs/viui/src/node_widgets.rs` | **CREATE** | — |
| `libs/viui/src/node_widgets/label.rs` | **CREATE** | `GLYPH_W/H` constants từ v1 label |
| `libs/viui/src/node_widgets/button.rs` | **CREATE** | `PAD`, paint logic tương tự v1 |
| `libs/viui/src/lib.rs` | **MODIFY** | add `pub mod node; pub mod node_widgets;` |
| `libs/viui/src/layout.rs` | READ ONLY | Constraints, Size, Rect, Point (reuse) |
| `libs/viui/src/event.rs` | READ ONLY | Event enum (reuse) |
| `libs/viui/src/canvas.rs` | READ ONLY | ViCanvas, Color, TextStyle (reuse) |

---

## Implementation Steps

1. Create `libs/viui/src/node.rs` — ViNode trait
2. Create `libs/viui/src/node_widgets/` directory (empty, for Law 5)
3. Create `libs/viui/src/node_widgets.rs` — `pub mod label; pub mod button;`
4. Create `libs/viui/src/node_widgets/label.rs`
5. Create `libs/viui/src/node_widgets/button.rs`
6. Add `pub mod node; pub mod node_widgets;` to `lib.rs`
7. `cargo check -p viui` — zero warnings

---

## Success Criteria

- [ ] `cargo check -p viui` clean
- [ ] `Box<dyn ViNode>` compiles (trait object-safe — no generics in trait)
- [ ] `Label::new(signal)` compiles; `paint()` calls `canvas.draw_text()`
- [ ] `Button::new("Click", || { ... })` compiles; `event()` calls `on_click` on click

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `Box<dyn Fn()>` in Button not `Send` | Fine — single-threaded UI, no Send needed |
| ViNode trait not object-safe | All methods take `&mut self` or `&self`, no generics → safe |
| `Event` borrow in `event()` | `event: &Event` is immutable borrow, fine |
