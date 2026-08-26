# P10b — ViUI v2 Performance: 2 Medium-Effort Optimisations

**Goal**: +3-6% CPU on top of P10a's ~87-94% Slint embedded parity.

**Context**: P10a closed the easy wins. P10b targets two structural hot-paths that
require slightly more surgery but have high payoff per-pixel: eliminating the
per-frame `bounding_rect()` recompute in the executor, and replacing float clip
comparisons in `put_pixel` (called once per pixel of every blended primitive)
with pre-cached integer ranges.

---

## Phase Overview

| # | Phase | Status | Files |
|---|-------|--------|-------|
| 01 | [Pre-compute bounding_rect at record time](phase-01-precompute-bounds.md) | ✅ Done | `gpu_cmd.rs`, `executor.rs` |
| 02 | [Integer clip cache in put_pixel](phase-02-integer-clip-cache.md) | ✅ Done | `canvas.rs` |

Phases are independent — can execute in any order.

---

## Bottlenecks Addressed

| Phase | Hot path | Before | After |
|-------|----------|--------|-------|
| 01 | `CpuExecutor::execute()` damage filter | `cmd.bounding_rect()` called per command per frame | Pre-computed at record time; executor reads stored field |
| 02 | `put_pixel()` clip check | 4 `f32` casts + 4 float comparisons per pixel | 4 integer comparisons (cached i32 x0/y0/x1/y1) |

---

## Key Constraints

- No `libs/api/` or `libs/types/` changes — Law 1 not triggered
- No `unsafe` — all changes use safe Rust
- `GpuCommandBuffer` is internal to `libs/viui` — API change is safe
- `cargo check -p viui` + `cargo check -p viui-demo` must pass after each phase

---

## Expected Impact

| Metric | Before | After |
|--------|--------|-------|
| Damage-rect check per command | Recompute `bounding_rect()` every frame | Read cached `Option<Rect>` field — zero compute |
| `put_pixel` clip check | 4 f32 casts + 4 float cmp | 4 i32 cmp — no float conversion, no FPU on embedded |
| Overall est. perf | ~87-94% Slint | **~90-100% Slint embedded parity** |
