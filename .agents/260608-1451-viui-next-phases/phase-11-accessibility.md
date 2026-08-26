# Phase 11 — Accessibility + Keyboard Navigation

**Status:** Planned  
**Stage:** G2  
**Priority:** Low  
**Estimate:** 2-3 ngày  
**Depends on:** Phase 03 (ViOS integration), Phase 05 (widget completeness), Phase 07 (FlexBox)

---

## Context

G1 apps: robot/embedded — no a11y requirements (no screen reader, no keyboard nav needed).  
G2 apps: kiosk, desktop — need:
1. **Keyboard focus navigation** (Tab/Shift+Tab cycles through focusable widgets)
2. **Keyboard activation** (Space/Enter activates Button, CheckBox)
3. **Focus ring** visual indicator
4. **Screen reader hook** (basic: announce widget type + value on focus change)

---

## Part A — Focus ring

All focusable widgets check `state_store.focused(id)` — already tracked in `WidgetStateStore`.

Missing: **visual focus ring**. Add to paint() of Button, CheckBox, TextEdit, Slider:

```rust
if cx.state.focused(self.id) {
    let ring = Rect::from(bounds_cache.get()).inflate(2.0);
    cx.canvas.draw_rect_outline(ring, cx.theme.accent(), 2.0);
}
```

`ViCanvas::draw_rect_outline()` — add if missing (4 lines, draws 4 thin rects).

---

## Part B — Tab order + focus cycling

`FocusManager` in `state_store.rs` — check current API.

Add `tab_order: Vec<WidgetId>` built during layout pass:
- Widgets register themselves during layout if `focusable = true`
- `Tab` key → advance to next in tab_order
- `Shift+Tab` → retreat

```rust
// app_runner.rs — handle Tab in event dispatch
Event::KeyPress { key: KeyCode::Tab, modifiers } => {
    if modifiers.shift {
        self.focus.retreat(&self.tab_order);
    } else {
        self.focus.advance(&self.tab_order);
    }
}
```

---

## Part C — Keyboard activation

Buttons + CheckBoxes should activate on Space/Enter when focused:

```rust
// In Button::event():
Event::KeyPress { key: KeyCode::Enter | KeyCode::Space, .. }
    if cx.state.focused(self.id) => {
        if let Some(cb) = &self.on_click { cb(); }
        Response::Consumed
    }
```

Slider: Left/Right arrows change value by step (0.05):

```rust
Event::KeyPress { key: KeyCode::Left, .. }
    if cx.state.focused(self.id) => {
        let new = (*self.value.borrow() - 0.05).clamp(0.0, 1.0);
        self.value.set(new);
        Response::Consumed
    }
```

---

## Part D — Screen reader hook (minimal)

G2 minimal: emit text to stderr or via system log when focus changes:

```rust
// app_runner.rs — on focus change:
fn announce_focus(widget: &dyn ViNode, state: &WidgetStateStore) {
    // Each widget optionally implements accessibility_label()
    if let Some(label) = widget.accessibility_label() {
        viui_log!("Focus: {}", label);
    }
}
```

Add `fn accessibility_label(&self) -> Option<String>` default method to `ViNode`:

```rust
trait ViNode {
    // default: None (no announcement)
    fn accessibility_label(&self) -> Option<String> { None }
}
```

Button, CheckBox, Slider, TextEdit override this to return meaningful description.

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/viui/src/canvas.rs` | MODIFY — add draw_rect_outline |
| `libs/viui/src/node.rs` | MODIFY — accessibility_label() default method |
| `libs/viui/src/state_store.rs` | MODIFY — tab_order Vec, advance/retreat methods |
| `libs/viui/src/app_runner.rs` | MODIFY — Tab cycling, focus announce |
| `libs/viui/src/node_widgets/button.rs` | MODIFY — focus ring + keyboard activate |
| `libs/viui/src/node_widgets/checkbox.rs` | MODIFY — focus ring + keyboard toggle |
| `libs/viui/src/node_widgets/slider.rs` | MODIFY — focus ring + arrow key step |
| `libs/viui/src/node_widgets/text_edit.rs` | MODIFY — focus ring (already handles keyboard) |

---

## Success Criteria

- Tab cycles focus through Button → CheckBox → Slider → TextEdit in order
- Focused widget shows 2px accent-color focus ring
- Enter/Space activates focused Button
- Arrow keys step focused Slider by 0.05
- `accessibility_label()` on Button returns label text
