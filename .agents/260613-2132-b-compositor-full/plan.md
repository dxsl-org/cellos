---
title: "Track B — Compositor full: VirtIO GPU hardware cursor + integration test"
description: "VirtIO GPU hardware cursor syscall (GpuCursor=301), Law-5 virtio_gpu split, and QMP mouse-move integration test. Phase 01 (software sprite) already shipped."
status: ✅ complete
priority: P2
effort: 2 phases complete, 1 deferred (Phase 04 already shipped in tests/integration/tests/compositor-cursor.rs)
branch: main
tags: [g2, gui, compositor, virtio-gpu, cursor, law1]
created: 2026-06-13
updated: 2026-06-14
---

# Track B — Compositor Full

Independent G2 GUI track (no overlap with Tracks A/C/E). Builds on the COMPLETE
Grant-surface compositor (2026-06-09). Adds the visible mouse cursor and an
efficient GPU blit path.

## Verified Current State (file:line)

- Compositor IPC loop + render: `cells/services/compositor/src/main.rs`,
  `render.rs:93 render_frame()`, software memcpy blend → `sys_gpu_flush`.
- Cursor position **already tracked**: `input_handler.rs:104 update_cursor()`
  reads `buf[2..6]=x`, `buf[6..10]=y` from the 0x10-prefixed MouseMove frame;
  stored in `InputState.mouse_x/mouse_y` (`input_handler.rs:31-33`).
- **No cursor is ever drawn.** `render.rs` blends surfaces only; cursor sprite absent.
- Kernel GPU: `kernel/src/task/drivers/virtio_gpu.rs` — `GPU_CONTEXT`,
  `framebuffer()` raw CPU memcpy; `GpuFlush` handler at `syscall.rs:1654`.
- `GpuFlush = 300` in `libs/api/src/syscall.rs:70` (cost weight 26 @ :295).
- **virtio-drivers 0.7.5 already exposes hardware cursor**: `setup_cursor()`
  (gpu.rs:137, 64×64 `CURSOR_RECT`) and `move_cursor()` (gpu.rs:170). Driver
  also does the full RESOURCE_CREATE_2D→ATTACH_BACKING→SET_SCANOUT→
  TRANSFER_TO_HOST_2D→RESOURCE_FLUSH pipeline internally (`flush()` gpu.rs:127).
- Mouse event path is COMPLETE end-to-end: kernel VirtIO input
  (`virtio_input.rs:98 poll_events`, EV_REL/EV_ABS → opcode 1/2) →
  input service (`input/src/main.rs:138-147` apply_rel/apply_abs → MouseMove) →
  compositor. Only the *visual* cursor is missing.
- Test harness: `tests/integration/src/lib.rs` `boot_with_netdev()` (:613) already
  attaches `-device virtio-gpu-device` (:642), `-device virtio-keyboard-device`
  (:641), QMP socket (:644), and `send_qemu_key()` (:734) via `input-send-event`.
  **Gap: NO mouse/tablet device attached** — `run-gui.ps1:34` has
  `virtio-mouse-device`, the test harness does not.
- Existing GPU test: `boot.rs:136 gpu_framebuffer_initialises`. Keyboard e2e
  pattern: `boot.rs:1541 input_keyboard_e2e`.

## Phases

| # | Phase | Status | Effort | Blocks On |
|---|-------|--------|--------|-----------|
| 01 | [Software cursor sprite](phase-01-software-cursor-sprite.md) | ✅ complete | ~200 LOC | — |
| 02 | [VirtIO GPU hardware cursor](phase-02-virtio-gpu-hw-cursor.md) | ✅ complete | ~320 LOC | 01 ✅ |
| 03 | [GPU 2D DMA blit pipeline](phase-03-gpu-2d-dma-blit.md) | deferred (NO-OP) | ~0 LOC | — |
| 04 | [Cursor mouse-move integration test](phase-04-cursor-integration-test.md) | ✅ complete | 1 test | 01 ✅ |

## Dependency Graph & Status Summary

```
01 ✅ done ──────────────────────────────────────────────────► 04 ✅ done (already shipped)
01 ✅ done ──► 02 ✅ done (GpuCursor=301, Law-1 confirmed) ──► (hw cursor active)
03 deferred (DMA pipeline already exists — no-op confirmed)
```

- **01** ✅ **complete**: cursor_sprite.rs + composite_cursor() + damage tracking + probe all shipped.
- **02** ✅ **complete**: hardware cursor shipped. GpuCursor=301 syscall wired end-to-end; Law 1 confirmed ×2; 64×64 sprite uploaded at startup; software cursor fallback preserved.
- **03** intentionally deferred: the DMA blit pipeline already exists end-to-end (virtio-drivers setup_framebuffer/flush). Real inefficiency = full-screen flush on small dirty rects; only worth fixing if benchmarks prove it matters.
- **04** ✅ **complete**: `compositor_cursor_moves_on_mouse_event` test exists in `tests/integration/tests/compositor-cursor.rs:66`. Pre-existing test discovered in cook session; all setup (boot_with_pointer, send_qemu_mouse_abs, probe emitting) already in place since 2026-06-09 (Phase 01 Grant completion).

## File Ownership (remaining phases)

- **Phase 01** ✅ complete: `cursor_sprite.rs`, `render.rs`, `input_handler.rs`, `main.rs` — all done.
- **Phase 02** owns: `kernel/src/task/drivers/virtio_gpu/cursor.rs` (new — Law-5 split),
  `libs/api/src/syscall.rs` (**Law 1 — 2× confirm before edit**),
  `libs/ostd/src/syscall.rs`, `kernel/src/task/syscall.rs`,
  `cells/services/compositor/src/input_handler.rs` + `main.rs`.
  NOTE: Phase 02 must split `virtio_gpu.rs` → `virtio_gpu/mod.rs` + `virtio_gpu/cursor.rs`
  (Law 5) before adding cursor fns, to keep 02/03 files disjoint.
- **Phase 03** deferred: `virtio_gpu/blit.rs` (only if benchmark justifies it).
- **Phase 04** owns: `tests/integration/tests/boot.rs` (new test only — `boot_with_pointer` +
  `send_qemu_mouse_abs` already exist at `lib.rs:711`/`:812`).

## Key Risks (full per-phase in phase files)

- **R1 (High):** QMP `virtio-mouse-device` is **relative** (EV_REL). A rel event
  of `dx,dy` moves the cursor by a delta from its current logical position, which
  starts at (0,0). The test must compute the expected absolute position from the
  injected deltas, OR switch the harness to `virtio-tablet-device` (EV_ABS) for
  deterministic absolute coordinates. Mitigation in Phase 04.
- **R2 (Med):** Hardware cursor (`setup_cursor`) allocates a DMA page and issues
  a blocking control-queue request. Calling it on every MouseMove (`move_cursor`)
  is cheap, but `setup_cursor` must be called once. Lifetime handled in Phase 02.
- **R3 (Med):** Cursor sprite over-paints surface pixels; the area under the old
  cursor position must be repainted (damage) when the cursor moves, or a trail
  is left. Phase 01 damage strategy addresses this.
- **R4 (Law 1):** Phase 02 adds a syscall id → requires 2× user confirmation
  before editing `libs/api/`. Flagged in Phase 02.

## Resolution & Next Steps

**Phase 02 COMPLETE (2026-06-14):** GpuCursor=301 syscall wired end-to-end. Law-1 confirmed ×2. Hardware cursor active when VirtIO GPU cursor is supported; software cursor fallback when not.

**Phase 04 DISCOVERY (2026-06-14):** The integration test `compositor_cursor_moves_on_mouse_event` was pre-existing in `tests/integration/tests/compositor-cursor.rs` as a well-written, complete end-to-end test. All harness support (boot_with_pointer, send_qemu_mouse_abs, cursor probe emission) was already in place from Phase 01 completion (2026-06-09). Phase 04 is thus **already complete**.

**Pre-existing compile bug fixed:** `tests/integration/src/lib.rs:1139` in `boot_x86_nvme()` was missing `monitor: None` field in QemuRunner struct literal (field added when QemuRunner was extended). Fixed and verified.

**Track B status:** 
- Phases 01, 02, & 04 complete (software cursor sprite + hardware cursor + integration test)
- Phase 03 deferred (no-op; DMA pipeline already exists in virtio-drivers)

## Open Questions

1. Phase 02 hardware cursor: When implemented, confirm GpuCursor=301 syscall ID is not in conflict with any Phase 26+ allocation (law-1 gate).
2. Should the GPU DMA blit (03) fully replace the CPU memcpy, or be feature-gated
   so QEMU `-display none` headless still works? Recommend keep `flush()` as-is
   (driver already does TRANSFER_TO_HOST_2D); 03 may be a NO-OP if the existing
   `gpu.flush()` already performs the DMA transfer — verify before building 03.
