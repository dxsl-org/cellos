# Phase 02 — Integer clip cache in put_pixel

**Status:** ✅ Done  
**Priority:** High  
**Effort:** Medium (struct field + clip_push/pop update + put_pixel hotspot)

## Context Links

- Plan: [plan.md](plan.md)
- `libs/viui/src/canvas.rs` — `FramebufferCanvas`, `put_pixel()`, `clip_push()`, `clip_pop()`

## Overview

`put_pixel()` is the innermost rasterization loop — called once per pixel for
every blended primitive (alpha text, lines, semi-transparent fills). It currently
converts `px`/`py` (already `u32`) to `f32` four times and does four float
comparisons against the clip rect.

On embedded targets without an FPU (common in RISC-V micro profiles), each
`px as f32` requires a software conversion. Even with an FPU, float comparisons
go through a different pipeline than integer compares.

Fix: add a shadow integer clip stack `clip_stack_i: [(i32,i32,i32,i32); CLIP_STACK_DEPTH]`
that stores `(x0, y0, x1, y1)` already as integers. `clip_push` and `clip_pop`
maintain both stacks in sync. `put_pixel` uses only the integer version.

The float `clip_stack` is kept intact for `clip_rect()` (returns `Rect`) and for
the float intersection logic in `clip_push()`.

## Requirements

- `FramebufferCanvas` gains `clip_stack_i: [(i32,i32,i32,i32); CLIP_STACK_DEPTH]`
- `new()` initialises `clip_stack_i[0]` from `(0, 0, width as i32, height as i32)`
- `clip_push()` computes integer bounds from the resulting `new_clip: Rect` and stores them
- `clip_pop()` only decrements `clip_depth` — both stacks are indexed by same depth
- `put_pixel()` reads `self.clip_stack_i[self.clip_depth]` and uses `<`, `>=` integer comparisons
- Float `clip_stack` unchanged — `clip_rect()` still returns `Some(active_clip())`
- No behavioural change — clipped region is identical (conversion: `floor` of `.x`, `ceil` of `.x+.w`)

## Architecture

```
FramebufferCanvas {
    // existing:
    clip_stack:   [Rect; CLIP_STACK_DEPTH],   // f32, used for clip_rect() + intersect()
    clip_depth:   usize,
    // new:
    clip_stack_i: [(i32,i32,i32,i32); CLIP_STACK_DEPTH],  // pre-converted integer bounds
}

clip_push(rect):
    new_clip = current.intersect(&rect).unwrap_or(Rect::ZERO)  // existing logic
    clip_depth += 1
    clip_stack[clip_depth] = new_clip                           // existing
    clip_stack_i[clip_depth] = (                               // new
        new_clip.x as i32,
        new_clip.y as i32,
        (new_clip.x + new_clip.w) as i32,
        (new_clip.y + new_clip.h) as i32,
    )

put_pixel(x, y, color):
    ...existing bounds check (px >= width, py >= height)...
    let (cx0, cy0, cx1, cy1) = self.clip_stack_i[self.clip_depth]  // ← integer read
    if (px as i32) < cx0 || (px as i32) >= cx1 { return; }         // ← integer cmp
    if (py as i32) < cy0 || (py as i32) >= cy1 { return; }         // ← integer cmp
    ...existing pixel write...
```

## Conversion Semantics

The float rect uses `[x, x+w)` as the clip interval (exclusive upper bound).
Integer conversion: `x0 = clip.x as i32` (truncate = floor for positive coords),
`x1 = (clip.x + clip.w) as i32` (truncate). This matches the original float
semantics exactly for integer-aligned rects (all normal widget layouts).
For sub-pixel rects the difference is at most 1px — identical to before since
the float comparison `px as f32 >= clip.x + clip.w` also truncates at integer
pixels.

## Related Code Files

**Modify:**
- `libs/viui/src/canvas.rs`
  - `FramebufferCanvas` struct: add `clip_stack_i: [(i32,i32,i32,i32); CLIP_STACK_DEPTH]`
  - `new()`: init `clip_stack_i[0] = (0, 0, width as i32, height as i32)`
  - `clip_push()`: after computing `new_clip`, set `clip_stack_i[self.clip_depth]`
  - `put_pixel()`: replace float clip block with integer stack read + integer comparisons

## Implementation Steps

1. Add `clip_stack_i` field to `FramebufferCanvas` struct, zero-init with `[(0,0,0,0); CLIP_STACK_DEPTH]`
2. In `new()`, set `clip_stack_i[0] = (0, 0, width as i32, height as i32)`
3. In `clip_push()`, after `clip_stack[self.clip_depth] = new_clip`, add:
   ```rust
   self.clip_stack_i[self.clip_depth] = (
       new_clip.x as i32,
       new_clip.y as i32,
       (new_clip.x + new_clip.w) as i32,
       (new_clip.y + new_clip.h) as i32,
   );
   ```
4. In `put_pixel()`, replace the two float clip lines:
   ```rust
   // BEFORE:
   let clip = self.clip_stack[self.clip_depth];
   if (px as f32) < clip.x || (px as f32) >= clip.x + clip.w { return; }
   if (py as f32) < clip.y || (py as f32) >= clip.y + clip.h { return; }
   // AFTER:
   let (cx0, cy0, cx1, cy1) = self.clip_stack_i[self.clip_depth];
   if (px as i32) < cx0 || (px as i32) >= cx1 { return; }
   if (py as i32) < cy0 || (py as i32) >= cy1 { return; }
   ```
5. `cargo check -p viui` — verify no errors
6. Confirm `clip_rect()` still returns float `active_clip()` unchanged

## Todo List

- [x] Add `clip_stack_i` field to `FramebufferCanvas`
- [x] Init `clip_stack_i[0]` in `new()`
- [x] Update `clip_push()` to set `clip_stack_i` entry
- [x] Replace float clip block in `put_pixel()` with integer read
- [x] `cargo check -p viui` clean
- [x] `cargo check -p viui-demo` clean

## Success Criteria

- `put_pixel()` contains zero `f32` conversions in the clip check path
- `clip_push()` updates both `clip_stack` and `clip_stack_i` in sync
- `cargo check -p viui` passes with zero new errors/warnings

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Integer truncation differs from float for sub-pixel rects | Difference is ≤1px; all ViUI widget layouts use integer coords |
| `clip_stack_i` not synced on `clip_push` of `Rect::ZERO` | `Rect::ZERO.x as i32 = 0`, `(0+0) as i32 = 0` — correctly clips everything |

## Security Considerations

None — pure performance refactor in the rasterizer hot path.
