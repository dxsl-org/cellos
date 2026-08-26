# Phase P08 — Multi-window + Window Chrome

**Step**: 3 (Full Windows)  
**Priority**: P3  
**Status**: 📋 Planned  
**Effort est.**: 6-8 ngày  
**Depends on**: P07

---

## Context Links

- [cells/services/compositor/](../../cells/services/compositor/) — Compositor (surface management)
- [libs/ostd/src/display.rs](../../libs/ostd/src/display.rs) — ViSurface (Grant-backed)
- [docs/specs/14-viui.md](../../docs/specs/14-viui.md) §11 Phân tầng theo profile

---

## Overview

Desktop mode: nhiều cửa sổ, mỗi cửa sổ có titlebar + close/minimize/maximize buttons + resize. `WindowManager` trong App Cell quản lý nhiều `ViSurface`. Compositor vẫn không biết về window chrome — toàn bộ decoration được render bởi ViUI trong App Cell's own surface.

---

## Architecture

### Window Model

```
App Cell:
  WindowManager
  ├── Window { surf: ViSurface, chrome: WindowChrome, content: Box<dyn ViWidget> }
  ├── Window { surf: ViSurface, ... }
  └── ...
```

Mỗi window = 1 `ViSurface` (Grant buffer). Chrome (titlebar, border) là phần của ViUI render, không phải compositor. Compositor chỉ thấy N surfaces và blend chúng.

### WindowChrome
```rust
pub struct WindowChrome {
    title:     alloc::string::String,
    state:     WindowState,   // Normal | Maximized | Minimized
    drag_pos:  Option<Point>, // active drag offset
}

impl WindowChrome {
    pub fn paint(&self, canvas: &mut dyn ViCanvas, surf_w: u32, surf_h: u32);
    pub fn titlebar_rect(&self) -> Rect;     // hit area for drag
    pub fn close_btn_rect(&self) -> Rect;
    pub fn event(&mut self, e: &Event) -> Option<WindowEvent>;
}

pub enum WindowEvent { Close, Minimize, Maximize, DragMove { dx: f32, dy: f32 } }
```

### WindowManager
```rust
pub struct WindowManager {
    windows: alloc::vec::Vec<ManagedWindow>,
    focused: usize,
    comp_tid: usize,
}

impl WindowManager {
    pub fn open(&mut self, title: &str, w: u32, h: u32,
                content: Box<dyn ViWidget>) -> WindowId;
    pub fn close(&mut self, id: WindowId);
    pub fn event_loop(&mut self) -> !;
}
```

### Resize handles

8 resize zones (N/S/E/W/NE/NW/SE/SW corners). Hit-testing → cursor change. Resize = reallocate ViSurface với new dimensions (sys_grant_unregister + sys_grant_register).

### Taskbar primitives

Separate `ViSurface` cho taskbar (bottom strip, 48px high). `TaskbarCell` là một App Cell riêng, không phải phần của ViUI — nhưng ViUI cung cấp `TaskbarItem` widget.

---

## Implementation Steps

1. `WindowChrome` — paint titlebar + buttons + border
2. `WindowEvent` enum + hit testing
3. Drag logic: `MousePress` trên titlebar → track offset → `surf.move_to()`
4. Close/Minimize/Maximize handling
5. `ManagedWindow` struct (ViSurface + chrome + content)
6. `WindowManager` — open, close, focus, z-order via compositor RAISE_SURFACE
7. Resize zones + cursor feedback
8. Resize: realloc ViSurface + re-layout content
9. Event loop integration (sys_recv → route to focused window)
10. `cargo check` clean

---

## Todo

- [ ] WindowChrome (paint + hit test)
- [ ] WindowEvent enum
- [ ] Drag implementation (move_to on compositor)
- [ ] Close / Minimize / Maximize
- [ ] ManagedWindow struct
- [ ] WindowManager (open/close/focus)
- [ ] z-order via RAISE_SURFACE IPC
- [ ] Resize zones + realloc logic
- [ ] Event loop (sys_recv → dispatch)
- [ ] cargo check clean

---

## Success Criteria

- 2 windows open simultaneously, each renders independently
- Drag titlebar → window moves on screen (compositor DamageNotify)
- Close button → surface destroyed, app continues
- Resize corner → ViSurface reallocated, content re-layouts
- Focus click → RAISE_SURFACE brings window to front

---

## Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|-----------|
| ViSurface realloc on resize is expensive | Medium | Debounce resize events (only realloc after release) |
| Multi-surface IPC ordering | Medium | Process DETACH before new ATTACH |
| Compositor z-order + RAISE_SURFACE race | Low | Compositor serializes all messages |
| Window Chrome renders into app's own content area | Low | Reserve top 32px for chrome, clip content below |

---

## Next Steps

- P09 (future): Animation system (fade, slide transitions)
- P10 (future): Taskbar Cell
- P11 (future): System notification widget
