# Phase P05 — Immediate Mode Facade (egui-compatible)

**Step**: 2 (Widgets)  
**Priority**: P1  
**Status**: 📋 Planned  
**Effort est.**: 3-4 ngày  
**Depends on**: P04

---

## Context Links

- [docs/specs/14-viui.md](../../docs/specs/14-viui.md) §5 Dual-Facade API

---

## Overview

Wrap `WidgetTree` + widgets với `Ui` struct có API ~95% giống egui. Developer biết egui gọi `ui.label()`, `ui.button()`, `ui.text_edit_singleline()` mà không cần học gì mới. Behind the scenes, mỗi call reconcile với retained widget tree.

---

## Requirements

### API compatibility target (egui-compatible)
```rust
// Developer code — giống egui
fn build_ui(ui: &mut Ui) {
    ui.heading("Hello ViCell");
    ui.label("Welcome!");
    if ui.button("Click me").clicked() {
        // handle
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut self.name);
    });
    ui.checkbox(&mut self.checked, "Enable");
    ui.add(MyCustomWidget::new());
}
```

### Response API (egui-compatible)
```rust
impl Response {
    pub fn clicked(&self) -> bool;
    pub fn double_clicked(&self) -> bool;
    pub fn hovered(&self) -> bool;
    pub fn changed(&self) -> bool;   // text_edit, checkbox
    pub fn rect(&self) -> Rect;
}
```

---

## Architecture

### Ui struct
```rust
// libs/viui/src/ui.rs
pub struct Ui<'tree> {
    tree:    &'tree mut WidgetTree,
    cursor:  LayoutCursor,     // current layout position
    id_gen:  IdGenerator,      // auto-increment WidgetId
}

impl<'tree> Ui<'tree> {
    // Text
    pub fn label(&mut self, text: impl Into<String>) -> Response;
    pub fn heading(&mut self, text: impl Into<String>) -> Response;  // larger font
    pub fn monospace(&mut self, text: impl Into<String>) -> Response;

    // Input
    pub fn button(&mut self, text: impl Into<String>) -> Response;
    pub fn text_edit_singleline(&mut self, text: &mut String) -> Response;
    pub fn text_edit_multiline(&mut self, text: &mut String) -> Response;
    pub fn checkbox(&mut self, checked: &mut bool, label: &str) -> Response;

    // Layout
    pub fn horizontal(&mut self, f: impl FnOnce(&mut Ui));
    pub fn vertical(&mut self, f: impl FnOnce(&mut Ui));
    pub fn with_layout(&mut self, layout: Layout, f: impl FnOnce(&mut Ui));
    pub fn separator(&mut self);
    pub fn space(&mut self, pixels: f32);

    // Generic
    pub fn add(&mut self, widget: impl ViWidget + 'static) -> Response;

    // Scroll
    pub fn scroll_area(&mut self, f: impl FnOnce(&mut Ui)) -> ScrollAreaResponse;
}
```

### Reconciler (đơn giản nhất)

Không cần full VDOM diff. Với retained mode + auto-increment ID:
- Mỗi `ui.button()` call → tìm node với matching `WidgetId` trong tree
- Nếu đã tồn tại: update text/state → reuse
- Nếu chưa có: insert mới vào current layout position
- Sau frame: remove nodes không còn được gọi (orphan cleanup)

```rust
impl Ui<'_> {
    fn reconcile<W: ViWidget + 'static>(&mut self, widget: W) -> Response {
        let id = self.id_gen.next();
        self.tree.upsert(id, widget, self.cursor.current_rect())
    }
}
```

### ViApp runner

```rust
// libs/viui/src/elm.rs (extend)
pub fn run_app<A: ViApp>(app: A, surf: ViSurface) -> ! {
    let mut app = app;
    let mut tree = WidgetTree::new();
    loop {
        // 1. view → rebuild tree
        let element = app.view();
        tree.reconcile(element);
        // 2. layout
        tree.layout(Size { w: surf.width() as f32, h: surf.height() as f32 });
        // 3. paint nếu có dirty region
        if let Some(dirty) = tree.take_dirty() {
            let mut canvas = FramebufferCanvas::new(surf.pixels_mut(), surf.stride(), surf.width(), surf.height());
            tree.paint(&mut canvas);
            surf.damage(dirty.into());
        }
        // 4. recv event, dispatch
        // ... (sys_recv với timeout, dispatch to tree)
    }
}
```

---

## Related Code Files

**Create**:
- `libs/viui/src/ui.rs`

**Modify**:
- `libs/viui/src/elm.rs` — thêm `run_app` runner
- `libs/viui/src/lib.rs` — pub mod ui

---

## Implementation Steps

1. `IdGenerator` (atomic u32 counter, reset mỗi frame)
2. `LayoutCursor` (tracks current x/y position trong layout)
3. `Ui` struct + constructor
4. `ui.label()`, `ui.heading()`, `ui.monospace()`
5. `ui.button()`, `ui.checkbox()`
6. `ui.text_edit_singleline()`, `ui.text_edit_multiline()`
7. `ui.horizontal()`, `ui.vertical()`, `ui.separator()`, `ui.space()`
8. `ui.add()` generic
9. `ui.scroll_area()`
10. `run_app` runner trong elm.rs
11. `cargo check -p viui`

---

## Todo

- [ ] IdGenerator (per-frame counter)
- [ ] LayoutCursor
- [ ] Ui struct
- [ ] label / heading / monospace
- [ ] button + Response
- [ ] checkbox
- [ ] text_edit_singleline + multiline
- [ ] horizontal / vertical / separator / space
- [ ] add(widget) generic
- [ ] scroll_area
- [ ] run_app runner
- [ ] cargo check clean

---

## Success Criteria

```rust
// Test app compile và chạy:
struct MyApp { name: String, count: u32 }
impl ViApp for MyApp {
    type Message = ();
    fn view(&self) -> Element<()> { ... }
    fn update(&mut self, _: ()) {}
}

// Immediate mode test:
fn build_ui(ui: &mut Ui) {
    ui.label("Hello");
    if ui.button("Count").clicked() { ... }
    ui.text_edit_singleline(&mut name);
}
```

Cả hai compile + render đúng.

---

## Next Steps

→ P06: Theming (dark/light colors cho tất cả widgets)  
→ P07: Elm facade hoàn chỉnh (iced macros, Element builder pattern)
