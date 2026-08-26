# Phase 02 — Navigation / Router

**Status:** Planned  
**Wave:** G1.1 (parallel với P01, P03, P04)  
**Priority:** Critical  
**Estimate:** 2-3 ngày  
**Depends on:** OverlayLayer từ P01 (nếu muốn modal transitions) — nhưng core Router KHÔNG cần P01

---

## Context Links

- ViApp runner: `libs/viui/src/app_runner.rs`
- Signal system: `libs/viui/src/signal.rs`
- Node trait: `libs/viui/src/node.rs`
- AnimatedSignal: `libs/viui/src/animation.rs`

---

## Overview

Hiện tại ViUI chỉ có 1 root widget. App nhiều màn hình (settings, detail, dashboard) phải tự quản lý swap widget — không có abstraction. Phase này thêm:

1. **Router** — maps PageId → builder function, giữ navigation history
2. **StackNavigator** — push/pop pages, slide transition
3. **TabNavigator** — bottom tab bar, instant switch (không có history)

---

## Key Insights

- G1 embedded apps thường có pattern: Main screen → Settings → thay đổi giá trị → quay về. StackNavigator đáp ứng đủ.
- Navigation state là `Signal<Vec<PageId>>` (stack). Pop = xóa cuối. Push = thêm cuối.
- Page builder function `Fn() -> Box<dyn ViNode>` được gọi lazy khi navigate tới.
- Transition animation: slide left/right với AnimatedSignal<f32> offset. Khi animation done, page cũ được drop.
- TabNavigator không có history — chỉ `Signal<usize>` current tab index.
- Không dùng `Box<dyn Any>` cho params — G1 đủ với `Fn()` closures capture context.

---

## Architecture

### Router (core)

```
libs/viui/src/navigation/
├── router.rs      — Router<PageId: Hash+Eq+Clone>
├── stack_nav.rs   — StackNavigator wrapping Router
└── tab_nav.rs     — TabNavigator
libs/viui/src/navigation.rs  — pub use re-exports
```

```rust
pub struct Router<PageId: Hash + Eq + Clone + 'static> {
    routes:  HashMap<PageId, Box<dyn Fn() -> Box<dyn ViNode>>>,
    history: Signal<Vec<PageId>>,
    current: Signal<PageId>,
}

impl<PageId: ...> Router<PageId> {
    pub fn new(initial: PageId) -> Self
    pub fn register(&mut self, id: PageId, builder: impl Fn() -> Box<dyn ViNode> + 'static)
    pub fn push(&mut self, id: PageId)      // thêm vào stack, rebuild widget
    pub fn pop(&mut self) -> bool           // quay về trang trước
    pub fn replace(&mut self, id: PageId)  // swap không push history
    pub fn can_pop(&self) -> bool
    pub fn current_widget(&self) -> &dyn ViNode
}
```

### StackNavigator (ViNode)

```rust
pub struct StackNavigator<PageId: ...> {
    router:    Router<PageId>,
    current:   Box<dyn ViNode>,
    prev:      Option<Box<dyn ViNode>>,    // đang slide out
    slide_anim: Option<AnimatedSignal>,    // 0.0 = old fully visible, 1.0 = new fully visible
    direction: SlideDir,                   // Forward | Backward
}
```

StackNavigator implement ViNode:
- `layout()`: trả về kích thước của trang hiện tại
- `paint()`: nếu animation đang chạy, paint cả prev (translated left) và current (translated right)
- `event()`: forward tới current page. Nếu animation done, drop prev.

Shortcut functions trên ViApp:
```rust
impl ViApp {
    pub fn navigate_push<PageId>(&mut self, id: PageId) { ... }
    pub fn navigate_pop(&mut self) { ... }
}
```
→ Yêu cầu ViApp biết navigator type → dùng type parameter hoặc trait object `dyn Navigator`.

**Practical approach cho G1:** StackNavigator làm ViNode bình thường. App tự giữ `Arc<Mutex<Router>>` và gọi push/pop trong callbacks. Navigator được mount làm root widget.

### TabNavigator (ViNode)

```rust
pub struct TabNavigator {
    tabs:        Vec<TabEntry>,   // { label, icon, builder }
    active:      Signal<usize>,
    pages:       Vec<Option<Box<dyn ViNode>>>,  // lazy-init
    tab_bar:     Box<dyn ViNode>,               // bottom bar
}
```

Tab bar: horizontal Row của TabButton, highlight active tab. Page area = remaining space above tab bar.

---

## Related Code Files

### Tạo mới
- `libs/viui/src/navigation.rs` — pub use re-exports
- `libs/viui/src/navigation/router.rs` — Router<PageId>
- `libs/viui/src/navigation/stack_nav.rs` — StackNavigator<PageId>
- `libs/viui/src/navigation/tab_nav.rs` — TabNavigator

### Sửa
- `libs/viui/src/lib.rs` — `pub mod navigation`
- `libs/viui/src/app_runner.rs` — thêm navigate_push/navigate_pop helpers (optional convenience)

### Không sửa
- node.rs, signal.rs, widget files — Navigation là composition, không cần thay đổi lower layers

---

## Implementation Steps

1. **router.rs** — Router<PageId> struct, register + push + pop + replace + can_pop
2. **stack_nav.rs** — StackNavigator struct, ViNode impl, slide animation logic
3. **tab_nav.rs** — TabNavigator struct, TabEntry, TabButton sub-widget, ViNode impl
4. **navigation.rs** — re-exports tất cả
5. **lib.rs** — `pub mod navigation`
6. **Demo app** — tạo `cells/apps/nav-demo/` hoặc cập nhật robot-dashboard với Settings screen

---

## Todo List

- [ ] Tạo `libs/viui/src/navigation/` directory (router.rs, stack_nav.rs, tab_nav.rs)
- [ ] Implement Router<PageId>: register, push, pop, replace, can_pop
- [ ] Implement StackNavigator: ViNode với slide animation
- [ ] Implement TabNavigator: ViNode với tab bar + lazy page init
- [ ] Tạo `libs/viui/src/navigation.rs` re-export module
- [ ] Cập nhật `lib.rs`
- [ ] Unit test Router logic (push/pop/history invariants)
- [ ] Demo: robot-dashboard hoặc nav-demo app với 2+ screens

---

## Success Criteria

- `router.push(PageId::Settings)` → render Settings screen với slide-in animation
- `router.pop()` → slide back, history giảm 1
- `router.can_pop()` trả false khi ở root screen
- TabNavigator switch instant giữa tabs, active tab highlighted
- Page builders gọi lazy (trang không visible không được construct)
- `cargo check -p viui` pass

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Generic PageId type khó dùng với trait object | Medium | Offer non-generic version với `&'static str` page ID |
| Slide animation khi pop direction sai | Low | Reverse direction dựa trên `history.len()` delta |
| Memory: page không visible vẫn allocated | Medium | Tab pages: lazy init, Stack pages: drop prev sau animation done |
| EventCtx chưa có khi P01 chưa done | Low | Route StackNavigator trực tiếp qua EventCtx sau P01, trước đó dùng Signal-based API |

---

## Security Considerations

Không relevant — pure UI state machine. PageId không phải URL, không có injection risk.

---

## Next Steps

Sau P02: bất kỳ app nào có thể có multi-screen. Kết hợp với P01 Dialog cho confirmation trước khi navigate.
