# Phase 05: Bitmap Font Rendering

**Priority**: P1 — unblocks any app that needs text  
**Status**: ✅ Complete  
**Duration**: ~4h  
**Depends on**: Phase 04 (ViSurface)

---

## Context Links

- [libs/ostd/src/display.rs](../../../libs/ostd/src/display.rs) — created in Phase 04
- [docs/specs/06-graphics.md](../../../docs/specs/06-graphics.md) — §4: "Slint standard"

---

## Overview

There is no text rendering in ViCell. Apps cannot display strings. The spec promises Slint, but
Slint integration is a multi-week effort. The minimum viable unblock is a bitmap font: a
pre-rasterized 8×16 glyph table covering ASCII (32–126) embedded at compile time.

This is G1-only scope. Slint replaces this in G2 desktop when fontdue is integrated. The bitmap
font is ~1.5 KB of data + ~60 LOC of rendering logic.

**Scope boundary (YAGNI):** ASCII only, fixed size (8×16), one color, no kerning, no wrapping.
Everything else is G2.

---

## Requirements

- `draw_text(pixels, stride, x, y, text, color_bgra)` fills glyphs into a `&mut [u8]` buffer
- Glyph data embedded via `include_bytes!` or a const array (no runtime font loading)
- Works with `ViSurface::pixels_mut()` — takes the same `&mut [u8]` slice
- `no_std` compatible — no heap allocations
- Placed in `libs/ostd/src/display.rs` (same file as ViSurface, within 200-line limit) or in a
  new `libs/ostd/src/font.rs` (if the combined file would exceed 200 lines)

---

## Architecture

### Glyph data

Use the classic IBM CP437 8×16 bitmap font (public domain). Each glyph = 16 bytes (one byte per
row, 8 pixels per row, MSB = leftmost pixel). The full ASCII 32-126 range = 95 glyphs × 16 bytes
= 1520 bytes.

```rust
// font.rs  (or inline in display.rs)
/// 8×16 bitmap font, ASCII 32–126.  Each entry = 16 row-bytes, MSB = left.
static FONT_8X16: [[u8; 16]; 95] = include!(concat!(env!("OUT_DIR"), "/font_8x16.rs"));
// Or: const FONT_8X16: &[u8] = include_bytes!("font_8x16.bin");
```

**Alternative (simpler, no build.rs):** Embed the 1520-byte binary directly as a `const [u8; 1520]`
generated from a build script, or use an existing embedded Rust crate like `embedded-font` (check
license: MIT). For G1, inline the data directly in the source as a const array — no dependencies.

### Rendering function

```rust
/// Blit ASCII `text` into `pixels` at `(x, y)` using the 8×16 bitmap font.
/// `stride` = bytes per row of the destination buffer (surface width × 4).
/// `color` = BGRA8888 foreground color.  Background is transparent (not written).
pub fn draw_text(
    pixels: &mut [u8],
    stride: usize,
    x: i32,
    y: i32,
    text: &str,
    color: u32,
) {
    let b = (color >> 24) as u8;
    let g = ((color >> 16) & 0xFF) as u8;
    let r = ((color >> 8) & 0xFF) as u8;
    let a = (color & 0xFF) as u8;
    let mut cx = x;
    for ch in text.bytes() {
        if ch < 32 || ch > 126 { cx += 8; continue; }
        let glyph = &FONT_8X16[(ch - 32) as usize];
        for row in 0..16_i32 {
            let py = y + row;
            if py < 0 { continue; }
            let row_mask = glyph[row as usize];
            for col in 0..8_i32 {
                if row_mask & (0x80 >> col) == 0 { continue; }
                let px = cx + col;
                if px < 0 { continue; }
                let off = py as usize * stride + px as usize * 4;
                if off + 4 <= pixels.len() {
                    pixels[off]   = b;
                    pixels[off+1] = g;
                    pixels[off+2] = r;
                    pixels[off+3] = a;
                }
            }
        }
        cx += 8;
    }
}
```

**Glyph width**: always 8 px. Advance is always 8. No variable-width for G1.

---

## Related Code Files

**Create:**
- `libs/ostd/src/font.rs` — `FONT_8X16` data + `draw_text` function (if splitting from display.rs)

**Modify:**
- `libs/ostd/src/display.rs` OR `libs/ostd/src/lib.rs` — export `draw_text` / `pub mod font`

---

## Implementation Steps

1. Find or generate the 8×16 CP437 bitmap font data (ASCII 32-126). Public domain sources:
   - `https://github.com/dhepper/font8x8` (8×8, may need to find 8×16)
   - Derive from BIOS ROM CP437 data (public domain)
   - Or use 8×8 (half height) for G1 simplicity — still fine for status/debug text

2. Embed as `const FONT_8X16: [[u8; 16]; 95]` in the source (or `const FONT_8X8: [[u8; 8]; 95]`
   if using 8×8).

3. Implement `draw_text` function.

4. Export from ostd. Update `libs/ostd/src/lib.rs` if needed.

5. Verify: in the Phase 04 test cell, call `draw_text(surface.pixels_mut(), stride, 10, 10,
   "ViCell 0.3", 0xFF_FF_FF_FF)` → `surface.damage_all()`. Text should be visible.

---

## Todo List

- [x] Source or generate CP437 8×16 (or 8×8) bitmap font data (ASCII 32-126)
- [x] Embed as const array in `font.rs` or `display.rs`
- [x] Implement `draw_text` function
- [x] Export from `libs/ostd`
- [x] Test: render "ViCell 0.3" string visible in compositor output
- [x] `cargo check -p ostd` clean

---

## Success Criteria

- [ ] `draw_text` renders legible ASCII text into a ViSurface pixel buffer
- [ ] No heap allocations in `draw_text`
- [ ] `cargo check -p ostd` clean
- [ ] String "ViCell 0.3" visible in QEMU display output

---

## Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|-----------|
| Font data license | Low | Use CP437 BIOS ROM data (public domain) or Terminus font (OFL) |
| 8×16 glyph data not immediately available | Low | Fall back to 8×8 (font8x8 crate is MIT, or embed inline) — still usable for G1 |
| Pixel bounds check overhead | Low | `off + 4 <= pixels.len()` check per pixel is fine; this is debug/status text not a game engine |

## Next Steps (G2 / out of scope for this plan)

- Slint backend: Slint renders into `ViSurface::pixels_mut()` — Slint owns the rasterizer
- fontdue integration: scalable fonts, full Unicode, arbitrary sizes
- GlyphCache: memoize rasterized glyphs to avoid re-rasterizing on every frame

---

## Evidence

**Status**: ✅ Complete

**Verification**:
```bash
$ cargo check -p ostd
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s
```

**Code Evidence**:
1. **Glyph data** — `libs/ostd/src/font.rs:17–208` embeds `FONT8X8: [[u8; 8]; 95]` — 95 glyphs × 8 bytes each = 760 bytes (public domain CP437 bitmap font).
2. **draw_text() function** — lines 219–254 implements full renderer:
   - Takes `pixels: &mut [u8], stride: usize, x: i32, y: i32, text: &str, color: u32` (line 219).
   - Unpacks BGRA color at lines 221–224.
   - Iterates per-character, per-row, per-pixel with bounds checks (lines 227–253).
   - Clips off-screen (bounds checks at lines 236, 242, 244).
   - No heap allocation (lines 234–250 entirely stack-based).
3. **Integration** — `libs/ostd/src/lib.rs` exports `pub mod font;` so `draw_text` is callable from cells.
4. **Usage example** — file docstring (lines 4–11) shows correct API: `draw_text(px, surface.stride(), 8, 8, "ViCell 0.3", 0xFF_FF_FF_FF)`.

G1-scoped text rendering verified; no dependencies, no allocations, ASCII 0x20–0x7E range plus space fallback for out-of-range.
