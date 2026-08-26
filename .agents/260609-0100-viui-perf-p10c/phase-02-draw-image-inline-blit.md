# Phase 02 — draw_image: inline blit with opaque fast path

**Status:** 🔲 Planned  
**Priority:** High  
**Effort:** Low-Medium (loop replacement + opaque branch in `canvas.rs`)

## Context Links

- Plan: [plan.md](plan.md)
- `libs/viui/src/canvas.rs` lines 337-362 — `draw_image` implementation

## Overview

Current `draw_image` inner loop:

```rust
for dx in 0..(dst_x1 - dst_x0) {
    let sx = (dx_off + dx) as usize;
    let src_off = sy * src_stride as usize + sx * 4;
    if src_off + 3 >= pixels.len() { continue; }
    let src = Color::bgra(pixels[src_off], pixels[src_off+1], pixels[src_off+2], pixels[src_off+3]);
    self.put_pixel(dst_x0 + dx, dst_y, src);
}
```

`put_pixel()` redundancies here:
1. `x < 0` — never: `dst_x0 + dx >= 0` (clipped rect)
2. `px >= self.width` — never: within clipped rect
3. Integer clip check — never fails (within pre-clipped rect)
4. `off + 3 >= self.pixels.len()` — never for valid fb row

In addition, `Color::blend_over()` inside `put_pixel()` has an `if sa == 255`
early-exit for opaque pixels, but this is not exploited at the blit level to
skip the blend call entirely and use `copy_from_slice` for the full 4 bytes.

**Fix**: Inline the blit loop with an **opaque fast branch**:
- `src_alpha == 255`: `copy_from_slice` the 4 source bytes directly — zero blend work
- `src_alpha == 0`: skip the pixel (transparent) — zero write work
- else: inline blend (same formula as blend_over slow path)

This gives optimal throughput for the three common image types:
- Fully opaque sprites (UI icons): all pixels go through `copy_from_slice`
- Mask images (alpha only): transparent pixels skipped, colored pixels blended
- General RGBA: blend inline, no redundant checks

## Architecture

```rust
fn draw_image(&mut self, dest: Rect, pixels: &[u8], src_stride: u32) {
    let clip = self.active_clip();
    let clipped = match dest.intersect(&clip) { Some(r) => r, None => return };
    let dx_off = (clipped.x - dest.x) as i32;
    let dy_off = (clipped.y - dest.y) as i32;
    let dst_x0 = clipped.x as i32;
    let dst_y0 = clipped.y as i32;
    let dst_x1 = (clipped.x + clipped.w) as i32;
    let dst_y1 = (clipped.y + clipped.h) as i32;

    for dy in 0..(dst_y1 - dst_y0) {
        let sy = (dy_off + dy) as usize;
        let dst_y = dst_y0 + dy;
        if dst_y < 0 || dst_y as u32 >= self.height { continue; }
        let dst_row_off = dst_y as usize * self.stride as usize;
        for dx in 0..(dst_x1 - dst_x0) {
            let sx = (dx_off + dx) as usize;
            let src_off = sy * src_stride as usize + sx * 4;
            if src_off + 3 >= pixels.len() { continue; }
            let dst_off = dst_row_off + (dst_x0 + dx) as usize * 4;
            if dst_off + 3 >= self.pixels.len() { continue; }  // paranoia
            let sa = pixels[src_off + 3];
            if sa == 0 { continue; }  // fully transparent — skip
            if sa == 255 {
                // Opaque: direct 4-byte copy, zero blend work
                self.pixels[dst_off..dst_off+4].copy_from_slice(&pixels[src_off..src_off+4]);
            } else {
                // Partial alpha: inline blend
                let sa32 = sa as u32;
                let inv  = 255 - sa32;
                let dst  = u32::from_le_bytes([
                    self.pixels[dst_off], self.pixels[dst_off+1],
                    self.pixels[dst_off+2], self.pixels[dst_off+3],
                ]);
                let sb = pixels[src_off] as u32;
                let sg = pixels[src_off+1] as u32;
                let sr = pixels[src_off+2] as u32;
                let db = dst        & 0xFF;
                let dg = (dst >>  8) & 0xFF;
                let dr = (dst >> 16) & 0xFF;
                let out = ((sb * sa32 + db * inv) / 255)
                        | (((sg * sa32 + dg * inv) / 255) << 8)
                        | (((sr * sa32 + dr * inv) / 255) << 16)
                        | (255u32 << 24);
                self.pixels[dst_off..dst_off+4].copy_from_slice(&out.to_le_bytes());
            }
        }
    }
}
```

Note: The outer `dst_y` row guard (`dst_y < 0 || dst_y as u32 >= self.height`)
is retained — it guards the row offset, which is a real safety check.
The inner per-pixel checks (`x < 0`, `px >= width`, clip stack) are removed
since they're guaranteed by the pre-clip computation.

## Related Code Files

**Modify:**
- `libs/viui/src/canvas.rs` — replace `draw_image` body (lines 337-362)

## Implementation Steps

1. Locate `fn draw_image` in `canvas.rs` (currently lines 337-362)
2. Retain the pre-clip computation block (lines 337-345) — unchanged
3. Replace the inner loop with the inline blit (see Architecture above)
4. Pre-compute `dst_row_off = dst_y as usize * self.stride as usize` per row
5. Add `dst_off` per pixel using `dst_row_off + (dst_x0 + dx) * 4`
6. Three-way branch on `sa`: 0 (skip), 255 (copy_from_slice), else (inline blend)
7. `cargo check -p viui` — verify clean

## Todo List

- [ ] Replace `draw_image` inner loop with inline blit + opaque fast path
- [ ] Pre-compute `dst_row_off` per row (outside inner `dx` loop)
- [ ] Three-way `sa` branch: 0→skip, 255→copy_from_slice, else→inline blend
- [ ] `cargo check -p viui` clean
- [ ] `cargo check -p viui-demo` clean

## Success Criteria

- `draw_image` contains zero `put_pixel()` calls
- Opaque (a==255) pixels: `copy_from_slice` used, zero blend arithmetic
- Transparent (a==0) pixels: `continue` — zero write work
- `cargo check -p viui` passes with zero new errors/warnings
- Pixel output identical to previous `blend_over` path

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Blend formula drift | Derived from `Color::blend_over` body — same integer formula |
| `dst_off` OOB for boundary rows | Paranoia guard `if dst_off + 3 >= self.pixels.len() { continue; }` |
| `src_off + 3` OOB still needed | Retained — guards source slice bounds |
| Note on row copy direction | `&pixels[src_off..src_off+4]` is a 4-byte slice — safe with the `src_off + 3` guard |

## Security Considerations

None — pure performance refactor. No data-flow changes, no new external inputs.
