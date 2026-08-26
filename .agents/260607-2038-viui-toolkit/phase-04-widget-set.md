# Phase P04 — Basic Widget Set

**Step**: 2 (Widgets)  
**Priority**: P1  
**Status**: 📋 Planned  
**Effort est.**: 5-7 ngày  
**Depends on**: P01, P02, P03  
**Algorithm refs**: OrbTK (`State` lifecycle, `WidgetFlags` bitfield, `MouseBehaviorState` pattern) · iced (widget trait shape)

---

## Context Links

- [docs/specs/14-viui.md](../../docs/specs/14-viui.md) §3 Core Engine

---

## Overview

Implement 6 widget cơ bản: `Label`, `Button`, `TextEdit`, `Checkbox`, `ScrollArea`, `Image`. Tất cả implement `ViWidget` trait từ P01, render qua `ViCanvas` từ P02. Đây là widget set đủ để build real G2 apps.

---

## Widget Inventory

### Label
```rust
pub struct Label {
    text:  alloc::string::String,
    style: TextStyle,
    wrap:  bool,
}
impl ViWidget for Label { ... }
```
- `layout()`: measure text width/height từ GlyphAtlas hoặc bitmap font
- `paint()`: `canvas.draw_text()`
- `event()`: passthrough (Label không interactive)

### Button — OrbTK `MouseBehaviorState` pattern
```rust
pub struct Button {
    label:    Label,
    padding:  Padding,
}
```
Button không lưu hover/pressed state trong struct — state sống trong `WidgetStateStore` (keyed by WidgetId).

**OrbTK insight: `WidgetFlags` bitfield** thay vì enum state machine:
```rust
// Trong WidgetState (đã có trong P01 WidgetStateStore)
// hover: bool + pressed: bool + focused: bool → 3 bits
// Dùng bitfield vì state là tổ hợp: có thể hovered + focused đồng thời
pub struct WidgetFlags(u8);
impl WidgetFlags {
    pub const HOVERED:  u8 = 0b001;
    pub const PRESSED:  u8 = 0b010;
    pub const FOCUSED:  u8 = 0b100;
    pub fn has(&self, flag: u8) -> bool { self.0 & flag != 0 }
    pub fn set(&mut self, flag: u8) { self.0 |= flag; }
    pub fn clear(&mut self, flag: u8) { self.0 &= !flag; }
}
```

**OrbTK insight: decoupled event → message flow** (không send message trực tiếp trong event handler):
```rust
// event() sets flags and queues a message — does NOT call on_press inline
fn event(&mut self, cx: &mut EventCx, e: &Event) -> EventStatus {
    match e {
        Event::MousePress { pos, .. } if cx.widget_rect.contains(*pos) => {
            cx.state.get_mut(cx.widget_id).flags.set(WidgetFlags::PRESSED);
            cx.mark_dirty();
            EventStatus::Consumed
        }
        Event::MouseRelease { pos, .. } => {
            let was_pressed = cx.state.get(cx.widget_id).flags.has(WidgetFlags::PRESSED);
            cx.state.get_mut(cx.widget_id).flags.clear(WidgetFlags::PRESSED);
            if was_pressed && cx.widget_rect.contains(*pos) {
                cx.push_message(self.on_press.clone()); // OrbTK: send Action::Press
            }
            cx.mark_dirty();
            EventStatus::Consumed
        }
        // GlobalRelease: clear pressed even if pointer left widget during drag
        Event::MouseRelease { .. } => {
            cx.state.get_mut(cx.widget_id).flags.clear(WidgetFlags::PRESSED);
            EventStatus::Ignored
        }
        _ => EventStatus::Ignored,
    }
}
```

- `paint()`: reads `cx.state.get(id).flags` → picks color from theme (`button_normal/hovered/pressed`)

### TextEdit
```rust
pub struct TextEdit {
    text:     alloc::string::String,
    cursor:   usize,          // byte offset
    focused:  bool,
    changed:  bool,
}
```
- `layout()`: fixed height (font height + padding), fill available width
- `paint()`: border + background + text + cursor blink (via damage + timer)
- `event()`: KeyPress → insert/delete char, arrow keys → move cursor

### Checkbox
```rust
pub struct Checkbox { checked: bool, label: Label }
```
- `layout()`: 16×16 box + label gap + label width
- `paint()`: border square + checkmark (draw_line) + label
- `event()`: mouse click → toggle

### ScrollArea
```rust
pub struct ScrollArea {
    child:      Box<dyn ViWidget>,
    scroll_y:   f32,           // current scroll offset
    child_size: Size,          // cached from last layout
}
```
- `layout()`: child layout với infinite height constraint; own size = available size
- `paint()`: clip_push(self rect) → translate → child.paint() → clip_pop
- `event()`: scroll delta → update scroll_y; forward mouse events translated

### Image
```rust
pub struct Image {
    pixels: alloc::vec::Vec<u8>,   // BGRA raw
    w: u32, h: u32,
}
```
- `layout()`: natural size hoặc constrained
- `paint()`: `canvas.draw_image()`
- `event()`: passthrough

---

## Related Code Files

**Create**:
- `libs/viui/src/widgets/label.rs`
- `libs/viui/src/widgets/button.rs`
- `libs/viui/src/widgets/text_edit.rs`
- `libs/viui/src/widgets/checkbox.rs`
- `libs/viui/src/widgets/scroll_area.rs`
- `libs/viui/src/widgets/image.rs`
- `libs/viui/src/widgets.rs` (parallel to `widgets/` — Law 5)

---

## Implementation Steps

1. `widgets.rs` — re-export tất cả widgets
2. `label.rs` — Label + text measure
3. `button.rs` — Button state machine + Response
4. `checkbox.rs` — Checkbox + checkmark paint
5. `text_edit.rs` — TextEdit + cursor + keyboard handling
6. `scroll_area.rs` — ScrollArea + clip + translate
7. `image.rs` — Image blit
8. Update `lib.rs` — `pub mod widgets`
9. `cargo check -p viui` clean

---

## Todo

- [ ] widgets.rs module re-export
- [ ] Label (layout + paint)
- [ ] Button (state machine + paint + Response)
- [ ] Checkbox (toggle + checkmark)
- [ ] TextEdit (insert/delete + cursor + keyboard)
- [ ] ScrollArea (clip + translate + scroll)
- [ ] Image (blit)
- [ ] cargo check clean

---

## Success Criteria

- Mỗi widget implement `ViWidget` và compile clean
- `Button::clicked()` return true đúng 1 lần sau mouse press+release
- `TextEdit` insert/delete ASCII chars chính xác
- `ScrollArea` clip đúng (child không paint ra ngoài scroll region)

---

## Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|-----------|
| TextEdit cursor byte vs char offset | Medium | Dùng char_indices() thay vì byte index |
| ScrollArea translate → sai event coords | Medium | EventCx carry offset transform |
| Button state machine race (release outside) | Low | Track "pressed" state, release anywhere |

---

## Next Steps

→ P05: Theming (`ViTheme` trait, DarkTheme/LightTheme/KioskTheme)
