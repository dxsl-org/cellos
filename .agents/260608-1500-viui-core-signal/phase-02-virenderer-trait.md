# Phase 02 — ViRenderer Trait + FramebufferRenderer

**Plan**: [plan.md](plan.md)  
**Depends on**: Phase 01 (DirtyRect, Signal<T>)  
**Status**: ✅ Done  
**Estimated**: 1 hour

---

## Context

ViUI v1 dùng `ViSurface` + `damage_all()` trực tiếp trong `run_app()`.  
`ViRenderer` trait tách biệt rendering backend khỏi widget code — cho phép swap CPU/GPU mà không thay đổi widget logic.  
`FramebufferRenderer` là G1 impl: bọc `ViSurface` + `FramebufferCanvas`, giải quyết vấn đề lifetime của `FramebufferCanvas<'fb>` bằng closure pattern.

---

## Requirements

### Functional

- `ViRenderer` trait có hai method: `render()` và `size()`
- `render(damage, draw)` — execute paint closure với exclusive canvas access, submit frame sau
- `size() -> (u32, u32)` — surface dimensions
- `FramebufferRenderer::new(surf: ViSurface)` — G1 CPU impl
- Trait object-safe: `Box<dyn ViRenderer>` phải compile

### Non-Functional

- `no_std + alloc`
- `#![forbid(unsafe_code)]` — Law 4
- Không chạm `libs/api/` — không cần Law 1 confirmation
- `FramebufferCanvas<'fb>` borrow issue resolved bằng closure pattern (canvas tạo bên trong closure)

---

## Architecture

### Tại sao closure pattern?

`FramebufferCanvas<'fb>` borrow pixels từ `ViSurface` — nếu dùng `fn canvas(&mut self) -> &mut dyn ViCanvas` thì có self-referential lifetime problem (canvas borrow từ self.surf mà self cũng bị borrow).

Closure pattern giải quyết hoàn toàn:
```rust
// Trong render(), canvas tạo trực tiếp từ surf borrow — borrow giải phóng sau closure
let pixels = self.surf.pixels_mut();  // borrow surf
let mut canvas = FramebufferCanvas::new(pixels, stride, w, h);
draw(&mut canvas);                    // dùng canvas
// borrow surf giải phóng khi canvas drop
```

### Object-safety

```rust
// Generic method → NOT object-safe (cannot use as dyn ViRenderer):
fn render<F: FnMut(&mut dyn ViCanvas)>(&mut self, f: F)

// &mut dyn FnMut → object-safe:
fn render(&mut self, damage: Option<Rect>, draw: &mut dyn FnMut(&mut dyn ViCanvas))
```

### `renderer.rs` — Full implementation

```rust
// SPDX-License-Identifier: MIT
//! ViRenderer trait — abstract rendering backend for ViUI v2.
//!
//! G1: `FramebufferRenderer` (CPU software rasterizer via FramebufferCanvas)
//! G2+: GPU backend implementing the same trait; widget code unchanged

use crate::canvas::{FramebufferCanvas, ViCanvas};
use crate::layout::Rect;
use ostd::display::ViSurface;

// ─── ViRenderer ───────────────────────────────────────────────────────────

/// Abstract rendering backend.
///
/// Implementors provide a canvas for one paint pass and handle frame submission.
/// Object-safe: may be used as `Box<dyn ViRenderer>`.
///
/// # Contract
///
/// 1. Call `render()` once per frame, after collecting dirty rects.
/// 2. `draw` closure receives exclusive canvas access — do all painting here.
/// 3. `damage` is advisory for G2+ partial-flip; G1 ignores it (damage_all).
pub trait ViRenderer {
    fn render(&mut self, damage: Option<Rect>, draw: &mut dyn FnMut(&mut dyn ViCanvas));
    fn size(&self) -> (u32, u32);
}

// ─── FramebufferRenderer ─────────────────────────────────────────────────

/// G1 CPU renderer — wraps ViSurface + FramebufferCanvas.
pub struct FramebufferRenderer {
    surf: ViSurface,
}

impl FramebufferRenderer {
    pub fn new(surf: ViSurface) -> Self { Self { surf } }

    /// Unwrap the inner ViSurface (e.g. for IPC return after app exit).
    pub fn into_surf(self) -> ViSurface { self.surf }
}

impl ViRenderer for FramebufferRenderer {
    fn render(&mut self, damage: Option<Rect>, draw: &mut dyn FnMut(&mut dyn ViCanvas)) {
        let stride = self.surf.stride() as u32;
        let (w, h) = (self.surf.width(), self.surf.height());
        let pixels = self.surf.pixels_mut();
        let mut canvas = FramebufferCanvas::new(pixels, stride, w, h);
        draw(&mut canvas);
        // G1: always damage full surface; G2+ will pass `damage` to partial-flip API
        let _ = damage;
        self.surf.damage_all();
    }

    fn size(&self) -> (u32, u32) {
        (self.surf.width(), self.surf.height())
    }
}
```

### Usage pattern (for future widget layer)

```rust
// App main loop:
let mut renderer = FramebufferRenderer::new(surf);
let mut dirty = DirtyRect::new();

// Wire signal → dirty:
let count_bounds = Rect::new(10.0, 20.0, 100.0, 30.0);
let dirty_rc = Rc::new(RefCell::new(DirtyRect::new()));
let dirty_clone = Rc::clone(&dirty_rc);
let _sub = count_signal.subscribe(move || dirty_clone.borrow_mut().mark(count_bounds));

// Per frame:
let region = dirty_rc.borrow_mut().take();
if region.is_some() {
    renderer.render(region, &mut |canvas| {
        // paint only affected region
        canvas.fill_rect(count_bounds, theme.background);
        canvas.draw_text(count_bounds.origin(), &format!("{}", *count_signal.get()), style);
    });
}
```

---

## Related Code Files

| File | Action | Note |
|------|--------|------|
| `libs/viui/src/renderer.rs` | **CREATE** | ViRenderer trait + FramebufferRenderer |
| `libs/viui/src/lib.rs` | **MODIFY** | Add `pub mod renderer;` |
| `libs/ostd/src/display.rs` | **READ ONLY** | Verify ViSurface API: `width()`, `height()`, `stride()`, `pixels_mut()`, `damage_all()` |

---

## Implementation Steps

1. Read `libs/ostd/src/display.rs` → confirm `ViSurface` method signatures
2. Create `libs/viui/src/renderer.rs` with `ViRenderer` trait and `FramebufferRenderer`
3. Add `pub mod renderer;` to `libs/viui/src/lib.rs`
4. Run `cargo check -p viui` — zero warnings/errors

---

## Success Criteria

- [x] `cargo check -p viui` clean after adding renderer.rs
- [x] `Box<dyn ViRenderer>` compiles (trait is object-safe — `&mut dyn FnMut` is object-safe)
- [x] `FramebufferRenderer` wraps ViSurface without lifetime errors (closure pattern)
- [x] `render()` closure receives `&mut dyn ViCanvas` — all ViCanvas methods accessible

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| ViSurface API mismatch | Read display.rs first (Step 1) |
| `FramebufferCanvas::new` signature differs | Read canvas.rs impl to verify constructor |
| `damage_all()` not on ViSurface | Check display.rs; use `damage_rect(full_rect)` as fallback |
