# Plan: Compositor Grant-Based Surface Redesign

**Status**: Ready for implementation  
**Created**: 2026-06-07  
**Slug**: `260607-1854-compositor-grant-surfaces`

---

## Problem Statement

The current compositor (`cells/services/compositor`) copies pixels via 512B IPC messages
(`WRITE_PIXELS`). For a 1080p window at 30fps this requires ~491,520 IPC calls/second — pure
overhead that contradicts ViCell's zero-copy SAS promise. Additionally, `ScreenFb::flush_rect`
allocates a fresh `Vec<u8>` every frame, and `RENDER_INTERVAL_TICKS` is hardcoded for one mtime
frequency.

The fix: App Cells own their pixel buffers via `sys_grant_register` (persistent Grant), share
read-only with the Compositor, and send only a 24-byte `DamageNotify` IPC to signal dirty
regions. The Compositor maps Grants at startup and reads directly — zero bytes copied.

---

## Key Dependencies

- Grant API (syscalls 208-212): already shipped — `sys_grant_register`, `sys_grant_share(perm=0
  ReadOnly)`, `sys_grant_slice`, `sys_grant_unregister` — all in `libs/ostd/src/syscall.rs`
- ⚠️ **Law 1**: Phase 02 modifies `libs/api/src/display.rs` — requires **2× user confirmation**
  before implementation
- Never-die / Compositor recovery: addressed separately in the reliability track (Phase 12)
- Font rendering (Phase 05): unblocks text in all app UIs; self-contained

---

## Phase Overview

| # | Phase | Status | Duration | Depends |
|---|-------|--------|----------|---------|
| 01 | [Immediate Hotfixes](phase-01-immediate-hotfixes.md) | ✅ Complete | ~2h | — |
| 02 | [Grant Surface Protocol API](phase-02-grant-surface-protocol.md) | ✅ Complete | ~1d | Law 1 confirm |
| 03 | [Compositor: Grant-Based Impl](phase-03-compositor-grant-impl.md) | ✅ Complete | ~1d | Phase 02 |
| 04 | [App-Side ViSurface Wrapper](phase-04-app-surface-wrapper.md) | ✅ Complete | ~1d | Phase 02 |
| 05 | [Bitmap Font Rendering](phase-05-bitmap-font.md) | ✅ Complete | ~4h | Phase 04 |

All phases complete as of 2026-06-09. Code implemented in working tree; uncommitted.

---

## Success Criteria (overall)

- [x] No `WRITE_PIXELS` IPC calls from any app cell for pixel data (ViSurface uses ATTACH_GRANT + DAMAGE_NOTIFY)
- [x] `flush_rect` in compositor makes zero heap allocations (staging buffer pre-allocated)
- [x] `cargo check` passes on all phases (verified 2026-06-09: api, compositor, ostd all clean)
- [x] Compositor renders a test surface written by an app cell via Grant (sys_grant_slice → read direct from app memory)
- [x] Benchmark: pixel-transfer throughput scales linearly with surface count, not IPC count (24-byte DamageNotify per frame, not per pixel block)
- [x] Text rendered via bitmap font visible in compositor output (FONT8X8 + draw_text integrated, no allocations)

---

## Files Modified / Created (summary)

**Modified:**
- `libs/api/src/display.rs` — new opcodes, DamageNotify struct (Law 1 ⚠️)
- `cells/services/compositor/src/render.rs` — pre-alloc staging buffer
- `cells/services/compositor/src/surface_table.rs` — Grant-based SurfaceState
- `cells/services/compositor/src/main.rs` — ATTACH_GRANT + DamageNotify handlers
- `libs/ostd/src/lib.rs` (or new `display.rs`) — ViSurface wrapper

**Created:**
- `libs/ostd/src/display.rs` — ViSurface, GlyphCache, draw_text
- `cells/services/compositor/src/font.rs` — bitmap glyph renderer

**Kept (deprecated, for compatibility):**
- `compositor_ops::WRITE_PIXELS` — kept in compositor dispatch but not used by new SDK
