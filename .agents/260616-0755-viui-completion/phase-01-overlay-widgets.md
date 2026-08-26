# Phase 01 — Overlay Widgets: Dialog, DropDown, Toast

**Status:** Complete  
**Wave:** G1.1 (parallel với P02, P03, P04)  
**Priority:** Critical  
**Estimate:** 3-4 ngày  
**Depends on:** Không (ViApp + canvas đã có)

---

## Context Links

- Baseline: `libs/viui/src/app_runner.rs` (ViApp, tick loop)
- Canvas API: `libs/viui/src/canvas.rs`
- Node trait: `libs/viui/src/node.rs`
- Existing widgets: `libs/viui/src/node_widgets/`
- vi-compiler codegen: `tools/vi-compiler/src/codegen.rs` (map_element)

---

## Overview

Ba widget hiện chưa tồn tại nhưng cần thiết cho bất kỳ production app nào:
1. **Dialog/Modal** — blocking overlay: confirm, alert, custom content
2. **DropDown/Select** — expandable list chọn một giá trị
3. **Toast** — transient notification tự dismiss

Cả ba đều cần **OverlayLayer** — một cơ chế trong ViApp để render widget lên trên root widget, chặn input xuống layer dưới. Đây là architectural addition quan trọng nhất trong phase này.

---

## Key Insights

- ViApp hiện chỉ có `root: Box<dyn ViNode>`. Cần thêm `overlays: Vec<OverlayEntry>`.
- OverlayEntry = widget + z-order + input-blocking flag.
- Dialog dùng overlay blocking (input không qua root). DropDown popup dùng overlay non-blocking đối với target button nhưng blocking cho vùng còn lại. Toast dùng overlay non-blocking hoàn toàn.
- Toast cần timer: `toast_deadline: Option<u64>` (tick timestamp). ViApp::tick_with_dt update deadline.
- DropDown khi mở ra cần biết vị trí của trigger widget → dùng trigger_bounds: Rect passed vào khi show.
- Không dùng global mutable state — overlay API thuộc về ViApp instance (callback pattern).

---

## Architecture

### OverlayLayer (mới, trong app_runner.rs)

```rust
pub struct OverlayEntry {
    widget: Box<dyn ViNode>,
    blocking: bool,       // chặn input xuống layer dưới không
    dismiss_on_outside: bool, // click ngoài = dismiss
}

// Trong ViApp:
overlays: Vec<OverlayEntry>,
toast_queue: VecDeque<ToastEntry>,

pub fn push_overlay(&mut self, entry: OverlayEntry)
pub fn pop_overlay(&mut self)
pub fn clear_overlays(&mut self)
```

Input routing mới trong `ViApp::tick`:
1. Nếu `overlays` không rỗng và top overlay `blocking=true` → chỉ dispatch event vào overlay widget
2. Nếu overlay `dismiss_on_outside=true` + click ngoài bounds → pop_overlay
3. Toast overlays không blocking, render dưới cùng của overlay stack

### Dialog Widget

```
libs/viui/src/node_widgets/dialog.rs
```

```rust
pub struct Dialog {
    title:       String,
    message:     String,
    buttons:     Vec<DialogButton>,
    content:     Option<Box<dyn ViNode>>, // custom content
    // callbacks
    on_confirm:  Option<Box<dyn Fn()>>,
    on_cancel:   Option<Box<dyn Fn()>>,
}

pub struct DialogButton {
    label:   String,
    kind:    ButtonKind, // Primary | Secondary | Danger
    action:  Box<dyn Fn()>,
}

// Helper fns:
pub fn alert(app: &mut ViApp, title: &str, msg: &str, on_ok: impl Fn() + 'static)
pub fn confirm(app: &mut ViApp, title: &str, msg: &str,
               on_confirm: impl Fn() + 'static, on_cancel: impl Fn() + 'static)
```

Render: dimmed background (rgba(0,0,0,0.5) qua alpha blend), centered card, min 300×150px.

### DropDown Widget

```
libs/viui/src/node_widgets/dropdown.rs
```

```rust
pub struct DropDown {
    selected:  Signal<String>,
    items:     Vec<String>,
    expanded:  bool,          // nội bộ — không expose ra Signal để tránh re-layout
    popup:     Option<DropDownPopup>,
    on_change: Option<Box<dyn Fn(String)>>,
}
```

Khi click trigger: push popup overlay vào ViApp (cần &mut ViApp ref — pass qua EventCtx).
Khi chọn item: cập nhật `selected`, pop overlay, gọi on_change.

**EventCtx** (mới) — context passed vào `ViNode::event()`:
```rust
pub struct EventCtx<'a> {
    pub app: &'a mut ViApp,  // để widgets có thể push overlay
}
```
→ Breaking change nhỏ: `fn event(&mut self, e: &Event) -> EventStatus` cũ chuyển thành `fn event(&mut self, e: &Event, cx: &mut EventCtx) -> EventStatus`.

### Toast System

```
libs/viui/src/node_widgets/toast.rs
libs/viui/src/toast_manager.rs
```

```rust
pub enum ToastKind { Info, Success, Warning, Error }

pub struct ToastConfig {
    pub message:     String,
    pub kind:        ToastKind,
    pub duration_ms: u32,  // 0 = manual dismiss
}

// API trên ViApp:
pub fn show_toast(&mut self, config: ToastConfig)
```

Toast render: bottom-center, max-width 400px, fade-in/fade-out (dùng AnimatedSignal opacity), tự pop sau duration_ms. Nếu có nhiều toast: stack theo vertical với gap.

---

## Related Code Files

### Tạo mới
- `libs/viui/src/node_widgets/dialog.rs` — Dialog widget
- `libs/viui/src/node_widgets/dropdown.rs` — DropDown widget
- `libs/viui/src/node_widgets/toast.rs` — Toast widget + ToastEntry
- `libs/viui/src/overlay.rs` — OverlayEntry, OverlayLayer logic
- `libs/viui/src/event_ctx.rs` — EventCtx struct

### Sửa
- `libs/viui/src/app_runner.rs` — thêm overlays Vec, toast_queue, push/pop/clear, routing logic, show_toast
- `libs/viui/src/node.rs` — thêm EventCtx vào trait signature
- `libs/viui/src/node_widgets.rs` — pub use dialog::Dialog, dropdown::DropDown, toast::Toast
- `libs/viui/src/lib.rs` — pub mod overlay, pub mod event_ctx
- `tools/vi-compiler/src/codegen.rs` — thêm Dialog + DropDown vào map_element
- Tất cả 16 existing widgets — update `fn event()` signature (thêm `cx: &mut EventCtx`)

### Cập nhật tests
- `tools/vi-compiler/tests/codegen_tests.rs` — dialog_codegen, dropdown_codegen tests

---

## Implementation Steps

1. **event_ctx.rs** — Tạo EventCtx struct (chỉ có `app: &mut ViApp` reference, lifecycle safe với lifetime)
2. **overlay.rs** — OverlayEntry struct + OverlayLayer logic (push/pop/clear, input routing rules)
3. **app_runner.rs update** — thêm overlays field, update tick() input routing, thêm show_toast + toast timer tick
4. **node.rs update** — update ViNode::event signature. BREAKING: compile để thấy tất cả widgets cần update
5. **Update 16 existing widgets** — thêm `_cx: &mut EventCtx` param (unused trong phần lớn widgets)
6. **dialog.rs** — implement Dialog, AlertDialog builder, ConfirmDialog builder
7. **dropdown.rs** — implement DropDown với popup overlay mechanism
8. **toast.rs + toast_manager.rs** — ToastEntry, ToastWidget, animation fade
9. **codegen.rs update** — map Dialog, DropDown (Toast không cần DSL vì là imperative API)
10. **Tests** — codegen tests cho Dialog + DropDown, unit test OverlayEntry routing logic
11. **Integration** — cập nhật robot-dashboard: thêm 1 Confirm dialog, 1 DropDown mode select, 1 Toast notification

---

## Todo List

- [ ] Tạo `event_ctx.rs` với EventCtx struct
- [ ] Tạo `overlay.rs` với OverlayEntry + routing logic
- [ ] Update `app_runner.rs`: overlays, show_toast, toast timer
- [ ] Update `node.rs`: ViNode::event signature
- [ ] Update tất cả 16 widgets: event signature (batch, ít thay đổi logic)
- [ ] Implement `dialog.rs`: Dialog, alert(), confirm() helpers
- [ ] Implement `dropdown.rs`: DropDown với popup overlay
- [ ] Implement `toast.rs`: ToastWidget + fade animation
- [ ] Update `node_widgets.rs` + `lib.rs`: re-exports
- [ ] Update `codegen.rs`: Dialog + DropDown trong map_element
- [ ] Viết codegen tests (2 tests)
- [ ] Update robot-dashboard với demo Dialog + DropDown + Toast
- [ ] `cargo check -p viui` không warning

---

## Success Criteria

- `alert(app, "Title", "Message", || {})` hiển thị dialog, click OK dismiss
- `confirm(app, ...)` hiển thị 2 buttons, callback đúng được gọi
- DropDown click mở popup list, chọn item cập nhật Signal<String>
- Toast tự dismiss sau duration_ms, fade animation smooth
- Input đến root widget bị chặn khi Dialog mở
- Tất cả 16 widgets biên dịch với signature mới
- `cargo check` full workspace pass

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| EventCtx lifetime phức tạp | Medium | Dùng raw pointer hoặc split ViApp thành OverlayHandle riêng |
| Breaking change ViNode::event cascade | High | Batch update toàn bộ 16 widgets trong 1 commit |
| DropDown popup position tính sai | Low | Dùng Rect từ widget's cached bounds |
| Toast timer drift | Low | Dùng accumulated dt_ms trong ViApp, không absolute time |

---

## Security Considerations

Không có security implication — pure UI rendering logic. Callbacks là Box<dyn Fn()>, không escape khỏi Cell.

---

## Next Steps

Sau P01: P02 (Navigation) dùng OverlayLayer mechanism cho modal transitions. P05 (Virtual ListView) có thể dùng Toast để notify loading state.
