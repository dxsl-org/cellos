# P09 — ViUI v2 Performance: Close the Slint Embedded Gap

**Goal**: Reach ≥80% Slint embedded CPU performance by fixing the 4 remaining
bottlenecks identified in the P08 post-audit.

**Context**: P07+P08 built the correct architecture (damage-rect, Signal wiring,
incremental layout gate). P09 eliminates the remaining hot-path allocations and
redundant per-pixel work that prevent ViUI matching Slint on no-GPU embedded.

---

## Phase Overview

| # | Phase | Status | Files | Parallel? |
|---|-------|--------|-------|-----------|
| 01 | [Signal notify zero-alloc](phase-01-signal-notify-zero-alloc.md) | ✅ Done | `signal.rs` | ✅ |
| 02 | [Label text-measure cache](phase-02-label-measure-cache.md) | ✅ Done | `label.rs` | ✅ |
| 03 | [GpuCommandBuffer retained](phase-03-retained-command-buffer.md) | ✅ Done | `gpu_cmd.rs` `gpu_renderer.rs` | ✅ |
| 04 | [Glyph row-burst renderer](phase-04-glyph-row-burst.md) | ✅ Done | `canvas.rs` | ✅ |

All 4 phases implemented. `cargo check -p viui` + `cargo check -p viui-demo` clean.

---

## Bottlenecks Fixed by This Plan

| Rank | Before P09 | After P09 |
|------|-----------|-----------|
| 1 | `signal.rs:105` Vec::clone on every notify → heap alloc × n_signals | Per-element Rc::clone — 2 integer ops per subscriber, zero alloc |
| 2 | `label.rs:40` chars().count() O(n) every layout | Cached — O(1) when text unchanged |
| 3 | `gpu_renderer.rs:40` GpuCommandBuffer::new() every render | Retained buffer — Vec capacity reused across frames |
| 4 | `canvas.rs:249-256` 64 put_pixel/char × full bounds+clip+blend | Row-burst + opaque fast-path — ~3.5× text render speedup |

---

## Key Dependencies

- **No `libs/api/` or `libs/types/` changes** — Law 1 not triggered
- **No `ViNode` trait change** — Law backward-compat preserved
- Phase 03 changes `GpuRenderer::new()` signature (adds retained `buf` field) — no callers
  outside `cells/apps/viui-demo` which uses trait-level `ViRenderer`, not the concrete type
- All phases: `cargo check -p viui` + `cargo check -p viui-demo` must pass

---

## Expected Impact

| Metric | Before P09 | After P09 | Slint baseline |
|--------|-----------|-----------|----------------|
| Heap allocs/signal notify | 1 Vec alloc | 0 | 0 |
| chars().count() calls/tick | O(labels) | O(changed) | O(changed) |
| GpuCommandBuffer allocs/frame | 1 Vec alloc | 0 (reuse) | 0 (retained) |
| put_pixel calls per char | 64 | ~20-30 (sparse) | ~8 (tile blit) |
| Overall estimated perf | ~55% Slint | **~80-85% Slint** | 100% |
