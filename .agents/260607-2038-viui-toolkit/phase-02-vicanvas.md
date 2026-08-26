# Phase P02 — ViCanvas + DrawTarget Integration

**Step**: 1 (Core)  
**Priority**: P0 — blocking tất cả widget rendering  
**Status**: 📋 Planned  
**Effort est.**: 3-4 ngày  
**Depends on**: P01

---

## Context Links

- [libs/ostd/src/display.rs](../../libs/ostd/src/display.rs) — ViSurface (pixel buffer owner)
- [docs/specs/14-viui.md](../../docs/specs/14-viui.md) §4 ViCanvas

---

## Overview

Implement `ViCanvas` trait và `FramebufferCanvas` — lớp render duy nhất của ViUI. Widget gọi canvas primitives (`fill_rect`, `draw_text`, `clip_push`) thay vì tạo triangle meshes. `FramebufferCanvas` implement trực tiếp trên `&mut [u8]` qua `embedded-graphics DrawTarget`.

---

## Requirements

### Functional
- `ViCanvas` trait với: `fill_rect`, `draw_text`, `draw_line`, `draw_image`, `clip_push/pop`
- `FramebufferCanvas`: implement `ViCanvas` trên `DrawTarget` (embedded-graphics)
- Clip stack: tối đa 16 levels (SmallVec hoặc fixed array)
- Color: packed BGRA u32 (match VirtIO GPU format)
- `draw_text` dùng bitmap font (P03 sẽ add GlyphAtlas path)
- `PaintCx` wrapper expose `ViCanvas` cho widget

### Non-functional
- Zero heap allocation trong render path
- `fill_rect` performance: gần memory bandwidth (no per-pixel branch)
- `#![forbid(unsafe_code)]` — Law 4

---

## Architecture

### ViCanvas trait
```rust
// libs/viui/src/canvas.rs
pub trait ViCanvas {
    fn fill_rect(&mut self, rect: Rect, color: Color);
    fn draw_text(&mut self, pos: Point, text: &str, style: &TextStyle);
    fn draw_image(&mut self, rect: Rect, pixels: &[u8], stride: u32);
    fn draw_line(&mut self, a: Point, b: Point, color: Color, width: u8);
    fn clip_push(&mut self, rect: Rect);
    fn clip_pop(&mut self);
    fn clip_rect(&self) -> Option<Rect>;
}

pub struct Color(pub u32);  // BGRA packed, matches VirtIO GPU
impl Color {
    pub const fn bgra(b: u8, g: u8, r: u8, a: u8) -> Self;
    pub const TRANSPARENT: Self = Self(0x00000000);
    pub const WHITE: Self = Self(0xFFFFFFFF);
    pub const BLACK: Self = Self(0xFF000000);
}

pub struct TextStyle {
    pub color:   Color,
    pub size_px: u16,    // 0 = use bitmap font (8px)
}
```

### FramebufferCanvas
```rust
// Implement trực tiếp trên &mut [u8] — không cần embedded-graphics type overhead
pub struct FramebufferCanvas<'a> {
    pixels: &'a mut [u8],
    stride: u32,          // bytes per row
    width:  u32,
    height: u32,
    clip_stack: [Rect; 16],
    clip_depth: usize,
}

impl ViCanvas for FramebufferCanvas<'_> {
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        // Clip intersection, then row-by-row memcpy of packed BGRA
        // Hot path: no branch per pixel, use slice::fill on each row
    }
    // ...
}
```

**Note**: embedded-graphics `DrawTarget` dùng khi cần compatibility với embedded-graphics ecosystem (e.g., drawing images loaded as embedded-graphics `ImageRaw`). `FramebufferCanvas` implement `DrawTarget` separately.

`ViCanvas` cần thêm two accessors để `PaintCx::paint_text` blit glyph trực tiếp:
```rust
pub trait ViCanvas {
    // ... existing methods ...
    fn pixels_mut(&mut self) -> &mut [u8];  // raw pixel buffer for GlyphAtlas::blit
    fn stride(&self) -> u32;                 // bytes per row
}
```
`FramebufferCanvas` exposes these trivially via its `pixels: &mut [u8]` and `stride: u32` fields.

### PaintCx
```rust
// libs/viui/src/widget.rs (add)
pub struct PaintCx<'a> {
    pub canvas:  &'a mut dyn ViCanvas,
    pub origin:  Point,    // widget's top-left in screen coords
    pub glyph_atlas: Option<&'a mut GlyphAtlas>,  // None = bitmap only
}
```

---

## Related Code Files

**Create**:
- `libs/viui/src/canvas.rs`

**Modify**:
- `libs/viui/src/widget.rs` — add `PaintCx`
- `libs/viui/src/lib.rs` — pub mod canvas

---

## Implementation Steps

1. `Color` type (u32 BGRA, const constructors, palette constants)
2. `TextStyle` struct
3. `ViCanvas` trait
4. `FramebufferCanvas` struct + clip stack
5. `fill_rect` — clipped row-by-row BGRA write
6. `draw_line` — Bresenham (integer, no float)
7. `draw_text` — delegate sang bitmap font (`ostd::font::draw_text`) cho size_px==0
8. `draw_image` — clipped blit (src pixels → dst pixels, row by row)
9. `clip_push` / `clip_pop` — Rect intersection with current clip
10. `PaintCx` struct
11. `cargo check -p viui` clean

---

## Todo

- [ ] Color type với BGRA packing + palette constants
- [ ] TextStyle struct
- [ ] ViCanvas trait (including pixels_mut + stride accessors for PaintCx glyph blit)
- [ ] FramebufferCanvas struct + [Rect; 16] clip stack
- [ ] fill_rect với clip + row-by-row write
- [ ] draw_line Bresenham integer
- [ ] draw_text bitmap path (delegate to ostd::font)
- [ ] draw_image clipped blit
- [ ] clip_push / clip_pop (Rect intersection)
- [ ] PaintCx trong widget.rs
- [ ] cargo check clean

---

## Success Criteria

- `fill_rect` full-screen 1920×1080 BGRA trong <5ms trên RISC-V QEMU (verify bằng mtime timer)
- `draw_text` bitmap mode render 80-char line ASCII chính xác
- Clip stack không panic với 16 levels
- Zero alloc trong render path (verified: không gọi `alloc::*` trong hot path)

---

## Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|-----------|
| Row-stride tính sai → pixel corruption | Medium | Unit test fill_rect với known pattern |
| BGRA vs RGBA mismatch với existing compositor | Low | Compositor dùng BGRA — match |
| Clip intersection sai → paint outside bounds | Medium | Saturating clamp, test với clipped rects |

---

## Next Steps

→ P03: GlyphAtlas (dùng `PaintCx.glyph_atlas` để scalable text trong `draw_text`)
