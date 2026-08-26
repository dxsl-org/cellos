# Phase 01 — fill_rect alpha: inline blend loop

**Status:** 🔲 Planned  
**Priority:** High  
**Effort:** Low (single loop replacement in `canvas.rs`)

## Context Links

- Plan: [plan.md](plan.md)
- `libs/viui/src/canvas.rs` lines 212-216 — `fill_rect` alpha slow path

## Overview

`fill_rect` alpha path (current):

```rust
} else {
    for y in y0..y1 {
        for x in x0..x1 { self.put_pixel(x, y, color); }
    }
}
```

`put_pixel()` executes (in order):
1. `x < 0 || y < 0` — always false: `x0 = clipped.x as i32 >= 0`
2. `px >= self.width || py >= self.height` — always false: clipped rect is bounded by screen
3. Integer clip check — always passes: loop range is exactly the pre-clipped rect
4. `off + 3 >= self.pixels.len()` — always false for valid framebuffer
5. Blend computation + write ← the only real work

Steps 1-4 are 100% redundant per pixel.

**Fix**: Inline the loop. Compute `row_off` once per row. Blend inline with the
same `Color::blend_over` logic. No unsafe, no behaviour change.

## Architecture

```rust
} else {
    // Alpha path: pre-clipped, so all bounds checks are redundant — blend inline.
    let sa = color.a() as u32;
    let inv = 255 - sa;
    for y in y0..y1 {
        let row_off = y as usize * self.stride as usize;
        for x in x0..x1 {
            let off = row_off + x as usize * 4;
            if off + 3 >= self.pixels.len() { continue; }  // paranoia guard
            let dst = u32::from_le_bytes([
                self.pixels[off], self.pixels[off+1],
                self.pixels[off+2], self.pixels[off+3],
            ]);
            let db = (dst        & 0xFF) as u32;
            let dg = (dst >>  8  & 0xFF) as u32;
            let dr = (dst >> 16  & 0xFF) as u32;
            let out = ((color.b() as u32 * sa + db * inv) / 255)
                    | (((color.g() as u32 * sa + dg * inv) / 255) << 8)
                    | (((color.r() as u32 * sa + dr * inv) / 255) << 16)
                    | (255 << 24);
            self.pixels[off..off+4].copy_from_slice(&out.to_le_bytes());
        }
    }
}
```

**Why expand blend_over inline?** Calling `Color::blend_over()` is already
`#[inline]`, but the early-exit `if sa == 255` check inside it is dead for this
path (we entered the `else` branch, so `sa != 255`). Inlining avoids the
redundant branch and exposes the full computation to LLVM for vectorisation.

**Note on paranoia guard**: `off + 3 >= self.pixels.len()` should never trigger
(clipped rect guarantees pixel addresses are valid), but it provides a safe
fallback without `unsafe` and costs nothing in practice (LLVM proves it away).

## Related Code Files

**Modify:**
- `libs/viui/src/canvas.rs` — replace `fill_rect` alpha `else` block (lines 212-216)

## Implementation Steps

1. In `canvas.rs`, locate the `fill_rect` `else` block (after `if color.a() == 255`)
2. Replace the `put_pixel` loop with the inline blend loop (see Architecture above)
3. Extract `sa` and `inv` from `color.a()` once before the outer loop
4. Inner loop: compute `row_off` per row, `off` per pixel, inline blend, `copy_from_slice`
5. `cargo check -p viui` — verify clean

## Todo List

- [ ] Replace `fill_rect` alpha `else` block with inline blend loop
- [ ] Verify `sa` / `inv` extracted before outer loop
- [ ] `cargo check -p viui` clean
- [ ] `cargo check -p viui-demo` clean

## Success Criteria

- `fill_rect` alpha path contains zero `put_pixel()` calls
- `cargo check -p viui` passes with zero new errors/warnings
- Pixel output unchanged (same blend formula as `Color::blend_over`)

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Blend formula drift from `Color::blend_over` | Derive directly from blend_over body — same integer formula |
| `off + 3` OOB for edge rows | Paranoia guard `if off + 3 >= self.pixels.len() { continue; }` |
| Inline makes code harder to read | Comment inline formula with reference to blend_over |
