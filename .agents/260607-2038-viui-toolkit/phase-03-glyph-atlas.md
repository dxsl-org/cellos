# Phase P03 — GlyphAtlas + fontdue Scalable Text

**Step**: 1 (Core)  
**Priority**: P1  
**Status**: 📋 Planned  
**Effort est.**: 3-4 ngày  
**Depends on**: P01, P02  
**Algorithm refs**: egui (`TextureAtlas`, `Galley` concept) · fontdue rasterizer

---

## Context Links

- [libs/ostd/src/font.rs](../../libs/ostd/src/font.rs) — Bitmap 8×8 (giữ nguyên)
- [docs/specs/14-viui.md](../../docs/specs/14-viui.md) §6 Text Rendering

---

## Overview

Thêm `GlyphAtlas` vào `libs/ostd/src/font_atlas.rs`. Dùng fontdue làm rasterizer, pre-warm ASCII tại app startup, cache hit = memcpy ~200 bytes (tốc độ bằng bitmap font). Bitmap font 8×8 giữ nguyên cho CLI mode và là fallback khi không có atlas.

---

## Requirements

### Functional
- `GlyphAtlas`: 512×512 alpha texture, shelf packing, BTreeMap cache
- `GlyphKey`: (codepoint: u32, size_px: u16) — không dùng float key
- `GlyphEntry`: atlas_x, atlas_y, w, h, advance, bearing_x, bearing_y
- `prewarm_ascii(font, size)` — rasterize 95 ASCII glyphs tại startup
- `get_or_insert(font, char, size)` — cache miss: rasterize + pack + insert
- `ViCanvas::draw_text` dùng atlas khi `PaintCx.glyph_atlas.is_some()`
- BGRA blending khi blit glyph lên framebuffer (alpha compositing đơn giản)

### Non-functional
- `no_std` + `alloc`
- Atlas: `Box<[u8]>` pre-allocated (512×512 = 256KB)
- Cache hit path: zero alloc
- `prewarm_ascii` chạy một lần — ~19ms chấp nhận được

---

## Architecture

```rust
// libs/ostd/src/font_atlas.rs
pub struct GlyphKey { pub codepoint: u32, pub size_px: u16 }
impl Ord for GlyphKey { /* lex order */ }

pub struct GlyphEntry {
    pub atlas_x:   u16,
    pub atlas_y:   u16,
    pub w:         u8,
    pub h:         u8,
    pub advance_x: i16,   // pixels to advance cursor
    pub bearing_x: i8,    // horizontal offset from cursor
    pub bearing_y: i8,    // vertical offset from baseline
}

pub struct GlyphAtlas {
    data:     Box<[u8]>,                        // 512×512 alpha
    entries:  BTreeMap<GlyphKey, GlyphEntry>,
    // Shelf packer state
    cursor_x: u16,
    cursor_y: u16,
    row_h:    u16,
}

impl GlyphAtlas {
    pub fn new() -> Self;   // alloc 256KB data + empty BTreeMap
    pub fn prewarm_ascii(&mut self, font: &fontdue::Font, size: f32);
    pub fn get_or_insert(&mut self, font: &fontdue::Font,
                         c: char, size: f32) -> Option<&GlyphEntry>;
    /// Blit một glyph lên destination buffer (alpha blend, BGRA)
    pub fn blit(&self, entry: &GlyphEntry,
                dst: &mut [u8], dst_stride: u32,
                x: i32, y: i32, color: u32);
}
```

### Shelf Packing (đơn giản nhất)

```
atlas 512×512:
  row 0: [A][B][C]...  height = max glyph height in row
  row 1: [a][b][c]...
  ...
  overflow: trả về None (atlas full)
```

Không cần bin packing phức tạp — ASCII + CJK cơ bản ở 16px vừa trong 512×512.

### `LayoutedText` — egui ref: `egui::Galley`

`GlyphAtlas::get_or_insert` cho biết kích thước một glyph, nhưng widget cần đo toàn bộ text trước khi paint (để `layout()` trả về đúng `Size`). egui giải quyết với `Galley` — pre-computed layout của một đoạn text.

```rust
// libs/ostd/src/font_atlas.rs (thêm)
pub struct GlyphPos {
    pub entry: GlyphEntry,
    pub x: f32,   // advance cursor position
    pub y: f32,
}

// egui ref: Galley — laid-out text ready to paint
pub struct LayoutedText {
    pub glyphs: alloc::vec::Vec<GlyphPos>,
    pub size:   Size,    // bounding box (width × line_height)
    pub text:   alloc::string::String,
}

impl GlyphAtlas {
    // Measure + layout text — trả về LayoutedText
    // Widget gọi trong layout() để biết size
    // Widget lưu LayoutedText để dùng lại trong paint() — không measure 2 lần
    pub fn layout_text(&mut self, font: &fontdue::Font,
                       text: &str, size_px: f32) -> LayoutedText;

    // Paint từ pre-laid-out text
    pub fn paint_text(&self, lt: &LayoutedText,
                      dst: &mut [u8], stride: u32,
                      x: i32, y: i32, color: u32);
}
```

**Tại sao cần**: nếu widget gọi `measure()` trong `layout()` rồi gọi lại `draw_text()` trong `paint()`, đó là double work. `LayoutedText` cache kết quả measure — layout pass tạo nó, paint pass dùng lại.

### Text rendering split — P02 canvas vs P01 PaintCx

`FramebufferCanvas::draw_text` handles **bitmap 8×8 only** (no atlas access):
```rust
// canvas.rs — only bitmap path
fn draw_text(&mut self, pos: Point, text: &str, style: &TextStyle) {
    ostd::font::draw_text(self.pixels, self.stride as usize,
                          pos.x as i32, pos.y as i32,
                          text, style.color.0);
}
```

Scalable text uses **`PaintCx` methods** — atlas + font live in PaintCx, not in FramebufferCanvas:
```rust
// P01 PaintCx (widget.rs) — atlas path
impl<'a> PaintCx<'a> {
    /// Measure + position all glyphs. Cache result in widget to avoid double-measure.
    pub fn layout_text(&mut self, text: &str, size_px: f32) -> Option<LayoutedText> {
        let (font, atlas) = (self.font?, self.atlas.as_deref_mut()?);
        Some(atlas.layout_text(font, text, size_px))
    }
    /// Paint a pre-laid-out LayoutedText (from layout_text). No re-measure.
    pub fn paint_text(&mut self, lt: &LayoutedText, pos: Point, color: u32) {
        if let Some(atlas) = &self.atlas {
            atlas.paint_text(lt, self.canvas.pixels_mut(), self.canvas.stride(),
                             pos.x as i32, pos.y as i32, color);
        }
    }
}
```

**Design rationale**: `FramebufferCanvas` doesn't own a font or atlas — it's a stateless pixel writer. The font/atlas are owned per-frame by `PaintCx`, just like egui's `Painter` holds a reference to the font system, not the canvas backend itself.

---

## Related Code Files

**Create**:
- `libs/ostd/src/font_atlas.rs`

**Modify**:
- `libs/ostd/src/lib.rs` — `pub mod font_atlas`
- `libs/viui/src/widget.rs` — add `PaintCx::layout_text()` + `PaintCx::paint_text()` (atlas path lives here)
- **`canvas.rs` NOT modified** — FramebufferCanvas only does bitmap; atlas path is in PaintCx

---

## Implementation Steps

1. `GlyphKey` + `GlyphEntry` structs
2. `GlyphAtlas::new()` — alloc 512×512 + empty map
3. Shelf packer: `pack(w, h) -> Option<(u16, u16)>`
4. `get_or_insert` — fontdue rasterize + pack + store
5. `blit` — alpha blend glyph alpha → BGRA destination
6. `prewarm_ascii` — loop 0x20..=0x7E, gọi get_or_insert
7. Add `PaintCx::layout_text()` + `PaintCx::paint_text()` to `libs/viui/src/widget.rs`
8. Update `libs/ostd/src/lib.rs` add `pub mod font_atlas`
9. `cargo check` cả `ostd` + `viui`

---

## Todo

- [ ] GlyphKey (Ord impl) + GlyphEntry structs
- [ ] GlyphAtlas::new() — 256KB pre-alloc
- [ ] shelf packer (pack fn)
- [ ] get_or_insert (fontdue rasterize + pack)
- [ ] blit (alpha composite glyph onto BGRA buffer)
- [ ] prewarm_ascii
- [ ] `GlyphPos` + `LayoutedText` structs — egui Galley ref
- [ ] `layout_text()` — measure + position all glyphs, return LayoutedText
- [ ] `paint_text()` — blit from pre-laid-out LayoutedText (no re-measure)
- [ ] PaintCx::layout_text() + PaintCx::paint_text() in widget.rs (atlas path — NOT in canvas.rs)
- [ ] font_atlas pub mod trong ostd/lib.rs
- [ ] cargo check clean

---

## Success Criteria

- `prewarm_ascii` tại 16px chạy trong <25ms (acceptable startup cost)
- Cache hit path: `get_or_insert` trả về `Some(&GlyphEntry)` không alloc
- `blit` render "Hello ViCell" chính xác (kiểm tra bằng screenshot hoặc pixel dump)
- Atlas không overflow với toàn bộ ASCII + 200 CJK glyphs ở 16px

---

## Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|-----------|
| Atlas 512×512 quá nhỏ cho CJK 24px | Medium | Tăng lên 1024×1024 nếu cần |
| fontdue rasterize không compile no_std | Low | Đã verify: fontdue crate hỗ trợ no_std |
| Alpha blending sai → text màu sai | Medium | Test với white text on black bg |

---

## Next Steps

→ P04: Basic Widget Set (Label dùng `draw_text`, Button dùng `fill_rect`)
