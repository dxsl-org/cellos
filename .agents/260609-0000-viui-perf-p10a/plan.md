# P10a — ViUI v2 Performance: 3 Quick-Win Micro-Optimisations

**Goal**: +6-11% CPU on top of P09's ~80-85% Slint embedded parity.

**Context**: P09 eliminated the major hot-path allocations and per-char overhead.
P10a targets the remaining measurable gains that are low-risk and self-contained:
(A) pixel read/write as u32 instead of 4 separate bytes, (B) DrawTextShort
coverage extended from 32→128 bytes, (C) Signal retain skipped when no handle
has been dropped.

---

## Phase Overview

| # | Phase | Status | Files | Parallel? |
|---|-------|--------|-------|-----------|
| 01 | [u32 pixel read/write](phase-01-u32-pixel-rw.md) | ✅ Done | `canvas.rs` | ✅ |
| 02 | [DrawTextShort 32→128 bytes](phase-02-drawtext-short-128.md) | ✅ Done | `gpu_cmd.rs` `gpu_canvas.rs` | ✅ |
| 03 | [Signal conditional retain](phase-03-signal-conditional-retain.md) | ✅ Done | `signal.rs` | ✅ |

All 3 phases implemented. Commit: adbc6506. `cargo check -p viui` + `cargo check -p viui-demo` clean.

---

## Bottlenecks Fixed

| Phase | Hot path | Before | After |
|-------|----------|--------|-------|
| 01 | `put_pixel`, `fill_rect`, `draw_text` fast path | 4 byte stores/loads per pixel | 1 `copy_from_slice`/`from_le_bytes` — LLVM lowers to single STR/LDR |
| 02 | `GpuCanvas::draw_text` | `String::from()` heap alloc for text >32B | Stack buffer up to 128B — covers all typical UI labels |
| 03 | `Signal::notify()` | `Vec::retain()` O(n) scan every notify | `retain()` only when `any_dead` flag set — zero cost per frame in steady state |

---

## Key Constraints

- No `libs/api/` or `libs/types/` changes — Law 1 not triggered
- No `unsafe` — all changes use safe `copy_from_slice` / `from_le_bytes` / `to_le_bytes`
- `GpuCmd::DrawTextShort` variant grows (stack cost: +96 bytes per command slot in retained buffer) — acceptable
- `cargo check -p viui` + `cargo check -p viui-demo` must pass after each phase

---

## Expected Impact

| Metric | Before | After |
|--------|--------|-------|
| Pixel write (opaque) | 4 stores × N pixels | 1 `copy_from_slice` × N |
| Text > 32B | `String::from()` heap alloc | stack buffer, zero alloc |
| `retain()` calls per frame | Every `signal.set()` | Only when handle dropped |
| Overall est. perf | ~80-85% Slint | **~87-94% Slint** |
