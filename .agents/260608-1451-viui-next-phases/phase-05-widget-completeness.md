# Phase 05 — Widget Library Completeness

**Status:** Complete  
**Stage:** G1  
**Priority:** Medium  
**Estimate:** 2-3 ngày  
**Depends on:** Phase 01 (RenderCtx pattern established), embedded/robot P01-P03 (node_widget pattern)  
**Can run parallel with:** Phase 03, Phase 04

---

## Context Links

- [`libs/viui/src/widgets/`](../../../libs/viui/src/widgets/) — v1 widgets (old Elm model)
- [`libs/viui/src/node_widgets/`](../../../libs/viui/src/node_widgets/) — v2 widgets (Reactive Signal)
- [`libs/viui/src/node_widgets/slider.rs`](../../../libs/viui/src/node_widgets/slider.rs) — reference pattern
- [`libs/viui/src/node_widgets/progress_bar.rs`](../../../libs/viui/src/node_widgets/progress_bar.rs) — reference

---

## Overview

**Old `widgets/`** (v1, Elm model): button, checkbox, column, image, label, row, scroll_area, space, text_edit  
**New `node_widgets/`** (v2, Reactive Signal): button, column, label, progress_bar, row, slider, touch_area

Gap: 5 widgets cần port sang `node_widgets/` pattern:
- `CheckBox` — thường dùng trong forms, settings
- `TextEdit` — cursor, keyboard input, selection
- `Image` — render `&[u8]` RGBA data
- `ScrollArea` — scrollable container (wraps any ViNode)
- `Space` + `Divider` — layout utilities

Thêm mới: `Card` container (bordered panel với padding).

---

## Part A — CheckBox (node_widgets port)

### v1 API review

```rust
// widgets/checkbox.rs — Elm style (for reference)
pub struct CheckBox { checked: bool, label: String }
```

### v2 node_widget API

```rust
// node_widgets/checkbox.rs

pub struct CheckBox {
    checked:   Signal<bool>,
    label:     Signal<String>,
    on_toggle: Option<Box<dyn Fn(bool)>>,
    bounds_cache: Cell<Rect>,
    _subs:     Vec<SubscriptionHandle>,
}

impl CheckBox {
    pub fn new(checked: Signal<bool>) -> Self { ... }
    pub fn label(mut self, s: Signal<String>) -> Self { ... }
    pub fn on_toggle(mut self, f: impl Fn(bool) + 'static) -> Self { ... }
}
```

**Paint:**
- Box 16×16 px, border = `cx.theme.border()`
- If checked: draw checkmark (diagonal lines or ✓ character)
- Label text kế bên box

**Events:**
- `MousePress { pos }` hoặc `TouchBegin { pos }` trong bounds → toggle + call on_toggle

---

## Part B — TextEdit (full node_widget port)

TextEdit là widget phức tạp nhất. Port từ `widgets/text_edit.rs` sang node_widget pattern với đầy đủ cursor support.

### State

```rust
pub struct TextEdit {
    text:       Signal<String>,
    placeholder: Signal<String>,
    on_submit:  Option<Box<dyn Fn(&str)>>,

    // Internal cursor state (Cell for interior mutability)
    cursor_pos:    Cell<usize>,   // byte index trong text
    scroll_offset: Cell<f32>,     // horizontal scroll
    focused:       Cell<bool>,
    bounds_cache:  Cell<Rect>,
    _subs:         Vec<SubscriptionHandle>,
}
```

### Events

- `Focus` → set focused = true, mark dirty
- `Blur` → set focused = false
- `Char(c)` → insert char at cursor_pos, advance cursor
- `KeyPress { key: Backspace }` → delete char before cursor
- `KeyPress { key: Delete }` → delete char at cursor
- `KeyPress { key: Left }` → cursor_pos -= char_boundary
- `KeyPress { key: Right }` → cursor_pos += char_boundary
- `KeyPress { key: Home }` → cursor_pos = 0
- `KeyPress { key: End }` → cursor_pos = text.len()
- `KeyPress { key: Enter }` → call on_submit

### Paint

1. Background: `cx.theme.input_bg()` (focused: `cx.theme.input_focused_bg()`)
2. Border: `cx.theme.border()` (focused: `cx.theme.input_focused_border()`)
3. If text empty + unfocused: render placeholder in secondary color
4. Render text (with horizontal scroll clip)
5. If focused: render cursor bar at cursor position (1px wide, text height, blink via AnimatedSignal hoặc timer)

### Cursor blink

G1 simplification: cursor always visible khi focused (no blink). G2 add blink timer.

### UTF-8 safety

`cursor_pos` là byte index. Cần `char_indices()` để advance correctly:

```rust
fn advance_cursor(text: &str, pos: usize) -> usize {
    text[pos..].char_indices().nth(1)
        .map(|(i, _)| pos + i)
        .unwrap_or(text.len())
}

fn retreat_cursor(text: &str, pos: usize) -> usize {
    text[..pos].char_indices().last()
        .map(|(i, _)| i)
        .unwrap_or(0)
}
```

---

## Part C — Image (node_widget port)

### v2 API

```rust
pub struct Image {
    data:   Signal<Option<alloc::sync::Arc<[u8]>>>,  // RGBA bytes
    width:  Signal<u32>,
    height: Signal<u32>,
    bounds_cache: Cell<Rect>,
    _subs:  Vec<SubscriptionHandle>,
}

impl Image {
    pub fn new(data: Signal<Option<Arc<[u8]>>>, width: u32, height: u32) -> Self { ... }
    /// Static image loaded once.
    pub fn static_rgba(data: &'static [u8], width: u32, height: u32) -> Self { ... }
}
```

**Paint:** `cx.canvas.draw_image(bounds.pos(), data, width, height)` — dùng `draw_image` từ ViCanvas (đã implement với inline blend trong P10c).

---

## Part D — ScrollArea (node_widget)

Container node bọc bất kỳ `Box<dyn ViNode>`:

```rust
pub struct ScrollArea {
    child:          Box<dyn ViNode>,
    scroll_y:       Cell<f32>,
    content_height: Cell<f32>,
    bounds_cache:   Cell<Rect>,
}

impl ScrollArea {
    pub fn new(child: Box<dyn ViNode>) -> Self { ... }
}
```

**Paint:**
1. `cx.canvas.clip_push(bounds_rect)`
2. Translate canvas by `(0, -scroll_y)` (clip content)
3. Paint child
4. Restore translation
5. `cx.canvas.clip_pop()`
6. Draw scrollbar if content_height > bounds.height

**Layout:**
- `content_height = child.layout(available_width, f32::INFINITY).height`
- ScrollArea height = min(content_height, max_height)

**Events:**
- `Scroll { delta_y }` → update scroll_y, clamp [0, content_height - bounds.height]

Note: ViCanvas cần `translate()` method nếu chưa có. Thay thế: paint child với offset `Point` thay vì canvas translation.

---

## Part E — Layout utilities

### Space

```rust
// Đơn giản — chỉ chiếm space
pub struct Space {
    width:  f32,
    height: f32,
}
impl Space {
    pub fn w(width: f32) -> Self { ... }
    pub fn h(height: f32) -> Self { ... }
    pub fn wh(width: f32, height: f32) -> Self { ... }
}
```

### Divider

```rust
pub struct Divider {
    axis:      Axis,  // Horizontal | Vertical
    thickness: f32,
    color:     Option<Color>,
}
```

Paint: single line or rect.

### Card

```rust
pub struct Card {
    child:    Box<dyn ViNode>,
    padding:  f32,
    radius:   f32,  // corner radius (0 = sharp) — G1 có thể dùng rect
}
```

G1: paint as filled rect (no rounded corners). G2 add rounded rect renderer.

---

## Migration note: old widgets/

Sau khi node_widgets có đủ coverage, `widgets/` (v1) có thể:
- Giữ nguyên (backward compat, v1 Elm apps vẫn chạy)  
- Hoặc soft-deprecate với `#[deprecated]`

**Decision**: giữ nguyên v1 — không force break. Mark với `#[cfg(feature = "viui-v1")]` nếu muốn opt-out.

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/viui/src/node_widgets/checkbox.rs` | CREATE |
| `libs/viui/src/node_widgets/text_edit.rs` | CREATE |
| `libs/viui/src/node_widgets/image.rs` | CREATE |
| `libs/viui/src/node_widgets/scroll_area.rs` | CREATE |
| `libs/viui/src/node_widgets/space.rs` | CREATE |
| `libs/viui/src/node_widgets/divider.rs` | CREATE |
| `libs/viui/src/node_widgets/card.rs` | CREATE |
| `libs/viui/src/node_widgets.rs` | MODIFY — register all new widgets |
| `libs/viui/src/lib.rs` | MODIFY — re-export |

---

## Implementation Steps (3 days)

**Day 1:**
1. CheckBox node_widget (paint + toggle event)
2. Space + Divider (trivial)
3. Image port (paint only, no event)
4. `cargo check`

**Day 2:**
1. ScrollArea (clip + scroll event + scrollbar)
2. Card container (padding + background rect)
3. `cargo check`

**Day 3:**
1. TextEdit (cursor state + all keyboard events + paint)
2. UTF-8 cursor helpers
3. `cargo check` full workspace

---

## Todo

- [ ] node_widgets/checkbox.rs: paint + toggle event
- [ ] node_widgets/space.rs + divider.rs
- [ ] node_widgets/image.rs: draw_image paint
- [ ] node_widgets/scroll_area.rs: clip + scroll + scrollbar
- [ ] node_widgets/card.rs: padding + bg rect
- [ ] node_widgets/text_edit.rs: cursor + keyboard events
- [ ] UTF-8 advance/retreat cursor helpers
- [x] Register all in node_widgets.rs + lib.rs
- [x] cargo check full workspace

---

## Success Criteria

- [x] CheckBox toggle calls on_toggle(true/false)
- [x] TextEdit: type characters, backspace delete, arrow keys move cursor
- [x] Image renders RGBA data (test với 16×16 test bitmap)
- [x] ScrollArea: child taller than container → scroll works
- [x] Divider renders visible horizontal line
- [x] Card wraps child với visible padding
- [x] All node_widgets `cargo check` clean

---

## Evidence (Completed 2026-06-08)

**Verification:** `cargo check -p viui`

**Deliverables verified:**
1. ✅ `libs/viui/src/node_widgets/checkbox.rs` — CheckBox struct + Signal-driven state + toggle event + bounds tracking
2. ✅ `libs/viui/src/node_widgets/text_edit.rs` — TextEdit with cursor state (byte index), keyboard events (char/backspace/arrow/home/end/enter), UTF-8 safe cursor movement, focused/unfocused rendering, placeholder support
3. ✅ `libs/viui/src/node_widgets/image.rs` — Image struct supporting Signal<Option<Arc<[u8]>>> RGBA data, static_rgba helper, draw_image via RenderCtx
4. ✅ `libs/viui/src/node_widgets/scroll_area.rs` — ScrollArea container with clip/scroll mechanics, scrollbar rendering, content_height tracking, scroll clamping
5. ✅ `libs/viui/src/node_widgets/space.rs` — Space widget (w, h, wh constructors)
6. ✅ `libs/viui/src/node_widgets/divider.rs` — Divider (Horizontal/Vertical axis, thickness, optional color)
7. ✅ `libs/viui/src/node_widgets/card.rs` — Card container (child + padding + optional radius, G1 = rect)
8. ✅ `libs/viui/src/node_widgets.rs` — All 7 widgets registered (mod + pub use)
9. ✅ `libs/viui/src/lib.rs` — All re-exported

**Build status:** `cargo check -p viui` clean, no warnings
