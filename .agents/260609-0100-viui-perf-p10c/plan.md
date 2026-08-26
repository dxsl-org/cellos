# P10c — ViUI v2 Performance: Inline blend paths in fill_rect and draw_image

**Goal**: Eliminate redundant per-pixel bounds/clip checks in the two remaining
hot paths that still route through `put_pixel()` after pre-clipping.

**Context**: After P10a + P10b, `put_pixel()` itself is lean (integer clip,
u32 R/W). But `fill_rect` alpha and `draw_image` call it per pixel **after
already computing a pre-clipped rect** — so every `x < 0`, `y < 0`,
`px >= width`, `py >= height`, and integer clip check inside `put_pixel()` is
guaranteed to pass and is pure dead-branch overhead.

P10c inlines both paths, eliminating those checks and enabling LLVM to see the
full loop body for autovectorisation on ARM Neon / RISC-V V targets.

---

## Phase Overview

| # | Phase | Status | Files |
|---|-------|--------|-------|
| 01 | [fill_rect alpha inline blend](phase-01-fill-rect-alpha-inline.md) | ✅ Done | `canvas.rs` |
| 02 | [draw_image inline blit](phase-02-draw-image-inline-blit.md) | ✅ Done | `canvas.rs` |

Phases are independent — both touch only `canvas.rs`.

---

## Bottlenecks Addressed

| Phase | Hot path | Before | After |
|-------|----------|--------|-------|
| 01 | `fill_rect` alpha (semi-transparent fills) | `put_pixel()` per pixel — 4 redundant bounds checks | Inline loop, zero redundant checks |
| 02 | `draw_image` (icon/sprite blit) | `put_pixel()` per pixel — 4 redundant checks; full blend even for opaque src | Opaque src: 4-byte `copy_from_slice` direct; alpha src: inline blend |

---

## Key Constraints

- No `libs/api/` or `libs/types/` changes — Law 1 not triggered
- No `unsafe` — all changes use safe Rust
- `fill_rect` opaque fast path (P10a) is NOT touched — already optimal
- Behaviour must be identical: same pixel output as the `put_pixel()` path
- `cargo check -p viui` + `cargo check -p viui-demo` must pass

---

## Expected Impact

| Metric | Before | After |
|--------|--------|-------|
| Semi-transparent fill, N pixels | 4 branches × N | 0 redundant branches × N |
| Opaque image blit | Full blend_over (sa==255 early exit + u32 read) | Direct 4-byte `copy_from_slice` per pixel |
| Alpha image blit | Same blend, plus 4 redundant checks | Same blend, zero redundant checks |
| Overall est. perf | ~90-100% Slint | Consolidates embedded parity, enables LLVM autovec |
