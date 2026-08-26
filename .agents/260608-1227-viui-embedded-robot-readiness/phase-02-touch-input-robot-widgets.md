# Phase 02 — Touch Input + Robot-Essential Widgets

**Status:** Planned  
**Priority:** Critical — robot UI không dùng được nếu thiếu  
**Estimate:** 3-4 ngày  
**Parallel with:** Phase 01 (khác file set)

---

## Context Links

- [`libs/viui/src/event.rs`](../../../libs/viui/src/event.rs) — Event enum cần Touch variants
- [`libs/viui/src/widgets/`](../../../libs/viui/src/widgets/) — thêm 3 widget files mới
- [`libs/viui/src/node_widgets/`](../../../libs/viui/src/node_widgets/) — v2 Signal-driven versions

---

## Overview

Robot/embedded panels dùng touchscreen, không phải mouse. Cần:

1. **Touch events** trong `Event` enum + routing
2. **ProgressBar** — sensor level, battery, task completion
3. **Slider** — parameter tuning (speed, threshold, gain)
4. **TouchArea** — raw gesture capture, tap/swipe callbacks

---

## Requirements

**Touch events:**
- Multi-finger aware: `finger_id: u32`
- Position in screen coords: `Point`
- 3 lifecycle: `TouchBegin`, `TouchMove`, `TouchEnd`
- Mouse-to-Touch simulation cho testing trong non-touch environments

**ProgressBar:**
- Horizontal + Vertical orientation
- Signal<f32> value (0.0–1.0)
- Configurable colors: track + fill
- Optional label overlay (percentage text)
- No interaction (display only)

**Slider:**
- Horizontal orientation (vertical stretch goal)
- Signal<f32> value (0.0–1.0), settable
- Drag via mouse OR touch
- on_change callback
- Track + thumb visual

**TouchArea:**
- Capture all pointer/touch events in its bounds
- Callbacks: `on_tap`, `on_drag(delta: Point)`
- Does NOT consume keyboard events

---

## Architecture

### Event enum additions

```rust
// libs/viui/src/event.rs — thêm vào Event enum

/// Touch lifecycle — fired before mouse events on touchscreen devices.
/// `finger_id` disambiguates multi-touch contacts (0 = primary).
TouchBegin  { pos: Point, finger_id: u32 },
TouchMove   { pos: Point, finger_id: u32 },
TouchEnd    { pos: Point, finger_id: u32 },
```

Routing strategy: `TouchBegin/Move/End` dùng cùng BottomUp hit-testing như MousePress.
`TouchArea` widget consume chúng; nếu không có TouchArea, fall through đến noop.

`Event::pointer_pos()` cần match thêm Touch variants.

### ProgressBar

```rust
// libs/viui/src/node_widgets/progress_bar.rs
pub struct ProgressBar {
    value:       Signal<f32>,     // 0.0..=1.0
    track_color: Color,
    fill_color:  Color,
    orientation: Orientation,     // Horizontal | Vertical
    show_label:  bool,
    _sub:        Option<SubscriptionHandle>,
}

pub enum Orientation { Horizontal, Vertical }

impl ProgressBar {
    pub fn new(value: Signal<f32>) -> Self { ... }
    pub fn color(mut self, fill: Color) -> Self { ... }
    pub fn vertical(mut self) -> Self { ... }
    pub fn with_label(mut self) -> Self { ... }
}
```

Paint logic:
1. Fill track rect (entire bounds, track_color)
2. Fill fill rect (value * width, fill_color)
3. If show_label: draw_text_scaled center-aligned "XX%"

Layout: Fixed height (horizontal=20px default), fills width from parent constraints.

### Slider

```rust
// libs/viui/src/node_widgets/slider.rs
pub struct Slider {
    value:       Signal<f32>,        // 0.0..=1.0 (settable)
    on_change:   Option<Box<dyn Fn(f32)>>,
    dragging:    Cell<bool>,
    bounds_cache: Cell<Rect>,       // set by layout(), used by event()
    _sub:        Option<SubscriptionHandle>,
}
```

Event handling:
- `MousePress { pos }` trong bounds → dragging = true, update value
- `MouseMove { pos }` khi dragging → compute ratio từ pos.x vs track bounds → signal.set()
- `MouseRelease` → dragging = false
- `TouchBegin/Move/End { finger_id: 0 }` — same logic

Paint:
1. Track rect (full width, height=4px centered)
2. Thumb circle at `bounds.x + value * bounds.w` (radius 10px)

### TouchArea

```rust
// libs/viui/src/node_widgets/touch_area.rs
pub struct TouchArea {
    child:       Box<dyn ViNode>,
    on_tap:      Option<Box<dyn Fn()>>,
    on_drag:     Option<Box<dyn Fn(Point)>>,  // delta from press
    press_pos:   Cell<Option<Point>>,
}

impl TouchArea {
    pub fn new(child: Box<dyn ViNode>) -> Self { ... }
    pub fn on_tap(mut self, f: impl Fn() + 'static) -> Self { ... }
    pub fn on_drag(mut self, f: impl Fn(Point) + 'static) -> Self { ... }
}
```

Event logic:
- `MousePress` / `TouchBegin` → store press_pos, consume
- `MouseMove` / `TouchMove` → if press_pos set, compute delta, call on_drag, consume
- `MouseRelease` / `TouchEnd` → if delta < 8px (tap threshold) call on_tap, clear press_pos

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/viui/src/event.rs` | MODIFY — add TouchBegin/Move/End |
| `libs/viui/src/node_widgets/progress_bar.rs` | CREATE |
| `libs/viui/src/node_widgets/slider.rs` | CREATE |
| `libs/viui/src/node_widgets/touch_area.rs` | CREATE |
| `libs/viui/src/node_widgets.rs` | MODIFY — add 3 mod declarations + pub use |
| `libs/viui/src/lib.rs` | MODIFY — re-export new widgets |

---

## Implementation Steps

1. **event.rs**: Thêm `TouchBegin`, `TouchMove`, `TouchEnd` variants. Update `pointer_pos()`.
2. **progress_bar.rs**: Implement ViNode. `layout()` returns height=20px, fills width. `paint()` track + fill + optional label. `collect_dirty_handles()` subscribes value signal.
3. **slider.rs**: Implement ViNode. `layout()` returns height=24px (track 4px + thumb 20px headroom). `event()` handles drag state via `Cell<bool>`. `bounds_cache: Cell<Rect>` set in `layout()` for use in `event()`.
4. **touch_area.rs**: Implement ViNode. `layout()` delegates to child. `paint()` delegates to child. `event()` captures touch + mouse, fires closures.
5. **node_widgets.rs + lib.rs**: Add mod + pub use.
6. `cargo check` — fix tất cả errors.
7. Update `viui-demo` để demo ProgressBar (animating value) + Slider.

---

## Todo

- [ ] Thêm Touch variants vào event.rs, update pointer_pos()
- [ ] Tạo node_widgets/progress_bar.rs (horizontal + vertical, Signal<f32>)
- [ ] Tạo node_widgets/slider.rs (drag, on_change, bounds cache via Cell)
- [ ] Tạo node_widgets/touch_area.rs (on_tap + on_drag)
- [ ] Update node_widgets.rs barrel
- [ ] Update lib.rs re-exports
- [ ] cargo check pass
- [ ] Test ProgressBar value 0.0→1.0 renders correctly
- [ ] Test Slider drag updates Signal<f32>

---

## Success Criteria

- `ProgressBar::new(value_signal)` renders track + fill proportionally
- `Slider` drag từ 0 đến 1 bằng mouse, value signal cập nhật
- `TouchArea::on_tap` fires khi click trong bounds
- Tất cả 3 widget không panic khi paint với `RenderCtx` (từ Phase 01)
- `cargo check` zero errors

---

## Risk

**Cell<Rect> cho Slider bounds**: `layout()` và `event()` là `&self`, nên dùng `Cell<Rect>` để store
cached bounds from layout pass. Nếu Phase 01 chưa xong (RenderCtx chưa có), widget dùng
`canvas.draw_text()` tạm thời (backward compat fallback).

**Touch routing**: Nếu input cell chưa gửi TouchBegin events (chỉ có MousePress), Slider và
TouchArea vẫn work vì mouse drag cũng được handle. Touch là additive path, không phải replace.
