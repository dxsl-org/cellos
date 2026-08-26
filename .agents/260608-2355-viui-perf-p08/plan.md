# P08 — ViUI v2 Performance: Signal→DirtyRect + Incremental Layout + DrawText Dedup

**Goal**: Match Slint's embedded CPU performance — damage-rect rendering, O(m) layout
(m = changed widgets), near-zero per-frame heap allocations.

**Motivation**: After P07, `CpuExecutor` damage-rect filtering is dead code because
`app_runner` always passes `render(None, ...)`. Signals change labels but always
trigger a full layout + full repaint. `draw_text` allocates a `String` per call.

---

## Phase Overview

| # | Phase | Status | Priority |
|---|-------|--------|----------|
| 01 | [Signal→DirtyRect wiring](phase-01-signal-dirty-wiring.md) | ✅ Done | P1 — enables P02 |
| 02 | [Incremental layout gate](phase-02-incremental-layout.md) | ✅ Done | P2 — depends on P01 |
| 03 | [DrawText short-string dedup](phase-03-drawtext-short-string.md) | ✅ Done | P3 — independent |

Phases 01+02 are sequential (P02 refactors the same `app_runner` P01 rewires).
Phase 03 is independent — can execute in parallel with 01 or after.

---

## Key Dependencies

- `libs/viui/src/dirty.rs` — `DirtyRect` already fully implemented; add `DirtyRegion` alias
- `libs/viui/src/node.rs` — `ViNode` trait; add `collect_dirty_handles()` default method
- `libs/viui/src/app_runner.rs` — core rewiring target for P01+P02
- `libs/viui/src/gpu_cmd.rs` + `gpu_canvas.rs` + `executor.rs` — P03 targets
- **No `libs/api/` or `libs/types/` changes** — Law 1 not triggered

---

## Architecture (after P08)

```
Signal::set()
  └─► subscriber closure (set up by collect_dirty_handles after layout)
        └─► dirty_region.borrow_mut().mark(widget.bounds())

tick():
  events → layout_dirty?
     YES  → layout() → re-subscribe → mark_all()
     NO   → (skip layout)
  
  damage = dirty_region.take()   ← Some(partial) or Some(full) or None
  None  → skip frame
  Some  → renderer.render(damage, paint)
            └─► GpuCanvas records cmds
                  └─► CpuExecutor skips cmds outside damage ← finally activated!
```

---

## Success Criteria (overall)

- Signal change → partial repaint (only dirty widget bounds sent to renderer)
- Click event → full layout + full repaint (same as today, correct for moved widgets)
- No layout on signal-only tick (zero `layout()` calls if only text changed)
- `draw_text` for labels/buttons (≤32 bytes): zero heap alloc
- All three `cargo check`/`cargo clippy` targets pass: `viui`, `viui-demo`, `viui-macros`
