# Phase 01 — u32 Pixel Read/Write

**Plan**: [plan.md](plan.md)
**Status**: Planned
**Priority**: P1 — highest call frequency; on every set pixel in fill_rect, draw_text, draw_line

---

## Problem

Three locations in `canvas.rs` currently read or write 4 bytes separately:

### A — `put_pixel` READ (lines 171-176)
```rust
let dst = Color::bgra(
    self.pixels[off],
    self.pixels[off + 1],
    self.pixels[off + 2],
    self.pixels[off + 3],
);
```
4 byte loads → 4 register loads → 4 shift+OR ops in `bgra()`. A single u32 load
gets all 4 bytes at once; LLVM emits a single `LDR` on ARM.

### B — `put_pixel` WRITE (lines 178-181)
```rust
self.pixels[off]     = out.b();
self.pixels[off + 1] = out.g();
self.pixels[off + 2] = out.r();
self.pixels[off + 3] = out.a();
```
4 byte stores → 4 `STRB` instructions on ARM. `copy_from_slice` of a `[u8;4]`
from a packed u32 gives LLVM enough information to emit a single `STR`.

### C — `fill_rect` opaque fast path WRITE (lines 203-208)
Same 4-byte store pattern as B, repeated for every pixel of every filled rect.
Unlike `put_pixel`, no read (no blend), so only the write matters here.
Also: `color.b()`, `.g()`, `.r()` are computed per-pixel even though color is
constant across the rect. Pre-computing the `[u8;4]` once before the loop avoids
3 shift+mask ops per pixel.

### D — `draw_text` fast path WRITE (P09 code)
Same 4-byte store pattern per glyph pixel. Pre-computing `style.color.0.to_le_bytes()`
once before the char loop eliminates the byte extraction calls inside the loop.

---

## Solution

Use `u32::from_le_bytes` for READ and `copy_from_slice(&u32.to_le_bytes())` for WRITE.
Both are safe (no unsafe). LLVM with `-O2` / `-O3` (release) lowers these patterns
to single 32-bit load/store on aligned targets.

**Why `to_le_bytes()` is correct for BGRA:**
`Color(u32)` packs BGRA as `bits 0-7 = B, 8-15 = G, 16-23 = R, 24-31 = A`.
`u32::to_le_bytes()` on a little-endian target (all RISC-V + ARM that run ViCell)
returns `[bits0-7, bits8-15, bits16-23, bits24-31]` = `[B, G, R, A]`.
This is exactly the byte order written at `pixels[off..off+4]`. Bit-for-bit identical.

**Why pre-computing outside the loop matters:**
`color.b()` = `(self.0 & 0xFF) as u8` — 1 AND + 1 cast. Called 4 times per pixel.
`color.0.to_le_bytes()` — 1 call, result cached in a register-sized `[u8;4]`.
Inside the loop: just `copy_from_slice` with a constant source.

---

## Requirements

- Output pixel bytes identical to current for all inputs (opaque and blended)
- No `unsafe` code
- `cargo check -p viui` clean, no new warnings introduced
- `fill_rect`, `put_pixel`, `draw_text` fast path all updated

---

## Architecture

### `libs/viui/src/canvas.rs` — put_pixel READ (lines 171-176)

**Before:**
```rust
let dst = Color::bgra(
    self.pixels[off],
    self.pixels[off + 1],
    self.pixels[off + 2],
    self.pixels[off + 3],
);
```

**After:**
```rust
let dst = Color(u32::from_le_bytes([
    self.pixels[off],
    self.pixels[off + 1],
    self.pixels[off + 2],
    self.pixels[off + 3],
]));
```

`u32::from_le_bytes([b, g, r, a])` reconstructs exactly `Color::bgra(b,g,r,a)` because
`bgra()` = `(b as u32) | ((g as u32)<<8) | ((r as u32)<<16) | ((a as u32)<<24)`,
which equals `u32::from_le_bytes([b,g,r,a])` by definition.

---

### put_pixel WRITE (lines 178-181)

**Before:**
```rust
self.pixels[off]     = out.b();
self.pixels[off + 1] = out.g();
self.pixels[off + 2] = out.r();
self.pixels[off + 3] = out.a();
```

**After:**
```rust
self.pixels[off..off + 4].copy_from_slice(&out.0.to_le_bytes());
```

---

### fill_rect opaque fast path (lines 195-210)

**Before:** computed per-pixel inside loop.

**After:** precompute bytes once, use `copy_from_slice`:
```rust
if color.a() == 255 {
    let pixel = color.0.to_le_bytes();  // computed once
    for y in y0..y1 {
        if y < 0 || y as u32 >= self.height { continue; }
        let row_off = (y as u32 * self.stride) as usize;
        for x in x0..x1 {
            if x < 0 || x as u32 >= self.width { continue; }
            let off = row_off + (x as usize) * 4;
            if off + 3 < self.pixels.len() {
                self.pixels[off..off + 4].copy_from_slice(&pixel);
            }
        }
    }
}
```

Note: `color.a() == 255` is the guard, so `color.0.to_le_bytes()` includes the
correct `0xFF` alpha byte — no change in behavior vs the previous hardcoded `0xFF`.

---

### draw_text fast path (P09 code, ~lines 266-282)

**Before:** `style.color.b()`, `.g()`, `.r()` called per-pixel inside glyph loop.

**After:** precompute once before outer char loop:
```rust
let pixel = style.color.0.to_le_bytes();  // once before `for ch in text.chars()`
// ... inside fast path:
self.pixels[off..off + 4].copy_from_slice(&pixel);
```

---

## Related Code Files

**Modify:**
- `libs/viui/src/canvas.rs`
  - `put_pixel`: lines 171-181 (read + write)
  - `fill_rect` fast path: lines 195-210 (precompute + write)
  - `draw_text` fast path: pixel write per glyph

---

## Implementation Steps

1. `put_pixel` READ: replace `Color::bgra(4 bytes)` with `Color(u32::from_le_bytes([4 bytes]))`
2. `put_pixel` WRITE: replace 4 byte stores with `copy_from_slice(&out.0.to_le_bytes())`
3. `fill_rect` opaque path: add `let pixel = color.0.to_le_bytes();` before outer loop; replace 4 stores with `copy_from_slice(&pixel)`
4. `draw_text` fast path: add `let pixel = style.color.0.to_le_bytes();` before `for ch in text.chars()`; replace 4 stores with `copy_from_slice(&pixel)`
5. `cargo check -p viui` — verify no errors or new warnings

---

## Todo List

- [ ] put_pixel READ: Color(u32::from_le_bytes(...))
- [ ] put_pixel WRITE: copy_from_slice(&out.0.to_le_bytes())
- [ ] fill_rect: precompute pixel bytes + copy_from_slice
- [ ] draw_text fast path: precompute pixel bytes + copy_from_slice
- [ ] cargo check -p viui clean

---

## Success Criteria

- `put_pixel`, `fill_rect` opaque path, `draw_text` fast path: zero calls to `.b()`, `.g()`, `.r()`, `.a()` per pixel in these paths (verifiable by reading code)
- `cargo check -p viui` passes
- Pixel output byte-identical to before

---

## Risk

- **`to_le_bytes()` byte order**: verified — BGRA u32 in LE = [B,G,R,A] bytes = same as before.
- **`copy_from_slice` bounds**: same `off + 3 < pixels.len()` guard already in place; slice `[off..off+4]` is safe whenever `off + 3 < len`.
- **`fill_rect`: `0xFF` alpha vs `color.a()`**: the gate is `color.a() == 255`, so `to_le_bytes()` gives the same `0xFF` alpha byte as the old hardcoded `0xFF`. No behavior change.
- **LLVM optimization**: `copy_from_slice` of a 4-byte `[u8;4]` constant is a well-known LLVM pattern; emits `STR` on ARM in release builds. Debug builds may still emit 4 STRBs — acceptable.
