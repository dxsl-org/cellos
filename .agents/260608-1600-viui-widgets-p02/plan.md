# ViUI v2 P02 — viui-widgets: Typed Widget Structs + Macros

**Plan ID**: 260608-1600-viui-widgets-p02  
**Stage**: G2  
**Priority**: P0 — required before DSL compiler (P03)  
**Created**: 2026-06-08  
**Depends on**: [P01 Signal engine](./../260608-1500-viui-core-signal/plan.md) ✅ Done  
**Design Brief**: [.agents/brainstorms/260608-viui-nextgen-architecture.md](../brainstorms/260608-viui-nextgen-architecture.md)

---

## Mục tiêu

Xây dựng Layer 2 Rust API widget layer cho ViUI v2:

1. **`ViNode` trait** — v2 widget interface (không dùng `Msg` generic, không rebuild tree)
2. **Leaf widgets**: `Label` (Signal-driven text), `Button` (callback-based click)
3. **Container widgets**: `Column` (vstack) + `Row` (hstack)
4. **Macros**: `vstack!` / `hstack!`
5. **`ViApp` runner** — minimal tick loop: events → layout → render

**Goal cuối**: demo counter app chạy được — Button click → Signal update → Label re-render.

---

## Scope Boundary

### IN scope
- `ViNode` trait (layout + paint + event, không có Elm Msg)
- Label with `Computed<String>` / `Signal<String>` property
- Button with `on_click: Box<dyn Fn()>` callback
- Column + Row layout containers
- `vstack!` / `hstack!` `macro_rules!`
- `ViApp` minimal runner (tick-based, full repaint per frame)
- Wiring vào `libs/viui/src/lib.rs`

### OUT of scope (deferred)
- Fine-grained Signal → DirtyRect per widget (cần stable layout bounds — P04 work)
- TextEdit, Checkbox, ScrollArea, Image (v1 giữ nguyên, v2 widgets minimal)
- Theme integration (v2 widgets dùng hardcoded color cho đơn giản P02)
- Focus management
- DSL compiler (P03)

---

## Phase Table

| Phase | File | Nội dung | Status |
|-------|------|----------|--------|
| P01 | [phase-01-vinode-leaf-widgets.md](phase-01-vinode-leaf-widgets.md) | ViNode trait + Label + Button | ✅ Done |
| P02 | [phase-02-containers-app-runner.md](phase-02-containers-app-runner.md) | Column + Row + vstack!/hstack! + ViApp runner | ✅ Done |

P02 phụ thuộc P01 (cần `ViNode` + leaf widgets).

---

## Files Created/Modified

| File | Action |
|------|--------|
| `libs/viui/src/node.rs` | **CREATE** — ViNode trait |
| `libs/viui/src/node_widgets.rs` | **CREATE** — pub mod re-exports |
| `libs/viui/src/node_widgets/label.rs` | **CREATE** |
| `libs/viui/src/node_widgets/button.rs` | **CREATE** |
| `libs/viui/src/node_widgets/column.rs` | **CREATE** |
| `libs/viui/src/node_widgets/row.rs` | **CREATE** |
| `libs/viui/src/app_runner.rs` | **CREATE** — ViApp tick runner |
| `libs/viui/src/lib.rs` | **MODIFY** — 3 new pub mod entries |

Không chạm `libs/api/` hay `libs/types/` — không cần Law 1 confirmation.

---

## Key Design Decisions

### ViNode vs ViWidget (v1)

v1 `ViWidget` dùng `PaintCx` + `EventCx` + `WidgetStateStore` + Msg generic.  
v2 `ViNode` đơn giản hơn — trực tiếp hơn:

```rust
pub trait ViNode: 'static {
    fn layout(&mut self, constraints: Constraints) -> Size;
    fn bounds(&self) -> Rect;
    fn paint(&self, canvas: &mut dyn ViCanvas);
    fn event(&mut self, event: &Event) -> bool; // true = consumed
}
```

### Signal-driven Label

```rust
pub struct Label {
    pub text:   Signal<String>,  // or Computed<String>
    pub style:  TextStyle,
    bounds:     Rect,
}
// paint() calls canvas.draw_text using *self.text.get()
// No allocation per paint — text borrowed from Signal
```

### Callback-driven Button

```rust
pub struct Button {
    pub label:    alloc::string::String,
    pub on_click: Box<dyn Fn()>,
    hovered:      bool,
    pressed:      bool,
    bounds:       Rect,
}
// event() calls on_click() directly on confirmed click
// Returns true to stop event bubbling
```

### Full repaint for P02

Dirty-rect per-widget optimization deferred to P04.  
`ViApp::tick()` marks `dirty = true` on any click/event, renders full frame.  
Signal values drive widget content — repaint shows updated state.

### Macros

`macro_rules!` trong `lib.rs` (hoặc `macros.rs` nếu cần tách):
```rust
vstack!(Label::new(...), Button::new(...))
// → Column::new(vec![Box::new(label), Box::new(button)])
```
