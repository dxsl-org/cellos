# Phase 04 — Glyph Row-Burst Renderer

**Plan**: [plan.md](plan.md)
**Status**: Planned
**Priority**: P4 — biggest single-phase speedup; ~3.5× text render throughput

---

## Problem

`canvas.rs:249-256` — FramebufferCanvas `draw_text` inner loop:

```rust
for row in 0..8i32 {
    let bits = glyph[row as usize];
    for col in 0..8i32 {
        if bits & (0x80u8 >> col) != 0 {
            self.put_pixel(cx + col, cy + row, style.color); // 64 calls per char
        }
    }
}
```

`put_pixel` at lines 162-182 does per-call:
1. `cx + col` bounds check (left edge)
2. `cx + col` bounds check (right edge)
3. `cy + row` bounds check (top edge)
4. `cy + row` bounds check (bottom edge)
5. Clip rect intersection test
6. Alpha blend: 2 multiply + 2 divide (or shift) if `alpha != 255`

For a typical char: 64 `put_pixel` calls × 6 ops = 384 operations per character.
For a 15-char label: 5760 ops per label per repaint.

---

## Solution

Three independent micro-optimisations applied directly to `draw_text`:

### A — Pre-fetch clip rect + bounds ONCE per character (not per pixel)

Compute `clip_ok: bool` and `row_px_base: i32` once before the glyph row loop
instead of per-pixel:

```rust
// Clip: skip entire char if its bounding box doesn't intersect clip region
let glyph_rect = ... // (cx, cy, cx+8, cy+8) — computed per char
if !glyph_rect.intersects(self.clip) { cx += 8; continue; }
```

Saves 5 bounds checks × 64 = 320 integer comparisons per character for any
character that lies entirely within the clip.

### B — Pre-compute row byte offset ONCE per row (not per pixel)

```rust
let row_off = (py as u32 * self.stride) as usize;
// then per-col: self.buf[row_off + px as usize] = pixel;
```

One multiply per row (8 total) instead of one per pixel (64 total). Saves 56
multiply+adds per character.

### C — Empty-row early exit

```rust
if bits == 0 { continue; }
```

For sparse glyphs (space, `.`, `,`, `'`), the majority of rows are 0. Skip
entirely — 0 pixel writes.

### D — Opaque fast path (no alpha blend)

For `style.color.a == 255` (the common case in all ViUI demos), skip the full
RGBA blend formula and write the 4 bytes directly:

```rust
if style.color.a == 255 {
    // direct 4-byte write — no blend
    self.buf[row_off + px_off .. row_off + px_off + 4].copy_from_slice(&rgba);
} else {
    // existing blend path
}
```

Eliminates 2 multiply + 2 divide (or 2 shift) per set pixel for fully opaque text.

---

## Requirements

- `FramebufferCanvas::draw_text` modified only — no change to trait `ViCanvas`
- `put_pixel` not touched (still used for other callers)
- Output pixel values identical to current for same input
- `cargo check -p viui` clean
- No unsafe code (Law 4)

---

## Architecture

### `libs/viui/src/canvas.rs` — replace `draw_text` inner loop

**Current structure** (lines 238-259):
```rust
fn draw_text(&mut self, pos: Point, text: &str, style: TextStyle) {
    use ostd::font::FONT8X8;
    let mut cx = pos.x as i32;
    let cy = pos.y as i32;
    for ch in text.chars() {
        let code = ch as u32;
        let idx = if code >= 0x20 && code <= 0x7E {
            (code - 0x20) as usize
        } else {
            (b'?' - 0x20) as usize
        };
        let glyph = &FONT8X8[idx];
        for row in 0..8i32 {
            let bits = glyph[row as usize];
            for col in 0..8i32 {
                if bits & (0x80u8 >> col) != 0 {
                    self.put_pixel(cx + col, cy + row, style.color);
                }
            }
        }
        cx += 8;
    }
}
```

**After** (optimised — A+B+C+D):
```rust
fn draw_text(&mut self, pos: Point, text: &str, style: TextStyle) {
    use ostd::font::FONT8X8;
    let mut cx = pos.x as i32;
    let cy = pos.y as i32;
    let opaque = style.color.a == 255;
    let rgba = [style.color.r, style.color.g, style.color.b, style.color.a];
    let w = self.width as i32;
    let h = self.height as i32;

    for ch in text.chars() {
        let code = ch as u32;
        let idx = if code >= 0x20 && code <= 0x7E {
            (code - 0x20) as usize
        } else {
            (b'?' - 0x20) as usize
        };

        // A — skip char if entirely outside bounds/clip
        if cx >= w || cx + 8 <= 0 || cy >= h || cy + 8 <= 0 {
            cx += 8;
            continue;
        }

        let glyph = &FONT8X8[idx];

        for row in 0..8i32 {
            // C — skip empty rows (space char, etc.)
            let bits = glyph[row as usize];
            if bits == 0 {
                continue;
            }

            let py = cy + row;
            if py < 0 || py >= h { continue; }

            // B — row offset pre-computed once per row
            let row_off = (py as usize) * (self.stride as usize);

            for col in 0..8i32 {
                if bits & (0x80u8 >> col) != 0 {
                    let px = cx + col;
                    if px < 0 || px >= w { continue; }
                    let off = row_off + (px as usize) * 4;
                    // D — opaque fast path: no blend, direct write
                    if opaque {
                        self.buf[off..off + 4].copy_from_slice(&rgba);
                    } else {
                        self.put_pixel(px, py, style.color);
                    }
                }
            }
        }
        cx += 8;
    }
}
```

**Notes:**
- `self.stride` = `self.width` (pixels per row; each pixel = 4 bytes, offset computed explicitly)
- The existing `put_pixel` fallback is kept for the non-opaque path — correctness over duplication
- `self.buf` is `&mut [u8]` (framebuffer slice from ViRenderer) — direct slice write is safe per Law 4 (no unsafe)
- If `buf` is actually `Vec<u8>` (heap), `&mut [u8]` indexing works identically

**Stride check**: Read `canvas.rs` to confirm whether `stride` field exists or
needs to be added (may just use `self.width * 4` bytes/row). Adjust if needed.

---

## Related Code Files

**Modify:**
- `libs/viui/src/canvas.rs` — `FramebufferCanvas::draw_text` only

---

## Implementation Steps

1. Read `canvas.rs` lines 1-270 to confirm field names (`buf`, `width`, `height`, stride handling)
2. Identify exact line range of `draw_text` in `FramebufferCanvas`
3. Apply A+B+C+D optimisations as shown above (adjust for actual field names)
4. Verify opaque `copy_from_slice` produces same byte layout as `put_pixel` for `a==255` case
5. `cargo check -p viui` — no errors
6. `cargo check -p viui-demo`

---

## Todo List

- [ ] Read canvas.rs to confirm field names (buf, width, height, stride)
- [ ] Apply optimised draw_text (A+B+C+D)
- [ ] Verify byte layout consistency for opaque path
- [ ] cargo check -p viui passes
- [ ] cargo check -p viui-demo passes

---

## Success Criteria

- `draw_text` inner loop no longer calls `put_pixel` for opaque fully-in-bounds pixels
- Empty glyph rows (bits==0) produce zero pixel writes
- `cargo check -p viui` passes
- Pixel output unchanged for same input

---

## Performance Impact Estimate

| Path | Before | After |
|------|--------|-------|
| Opaque char, entirely in bounds | 64 put_pixel calls (384 ops) | ~20-30 direct writes (120-180 ops) |
| Space character (' ', 3-4 set bits, 5-6 empty rows) | 64 put_pixel calls | ~4-12 writes (5-7 empty rows skipped) |
| Full 15-char label @ 60Hz | ~5760 ops/repaint | ~1800-2700 ops/repaint |

Estimated: **~2.5-3× speedup** for typical opaque ASCII UI text.
Combined with P01+P02+P03: total ~**80% Slint embedded CPU parity**.

---

## Risk

- **`buf` layout mismatch**: If pixels are stored BGRA instead of RGBA, the
  `copy_from_slice(&rgba)` will show wrong colors. Check `put_pixel` source to
  confirm byte order, then adjust `rgba` array accordingly.
- **Non-opaque text in demos**: Current `viui-demo` uses `Color::WHITE` (a=255).
  The opaque fast path fires for all current demos. Fallback to `put_pixel` for
  semi-transparent text ensures correctness.
- **Stride assumption**: If `FramebufferCanvas` has no `stride` field and uses
  `width*4` internally, replace `self.stride as usize` with `self.width as usize * 4`.
