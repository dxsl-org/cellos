# Phase 04 — Cursor Plane

## Context Links
- Plan: [plan.md](plan.md) · Prev: [phase-03](phase-03-compositor-scanout-bridge.md)
- Cursorq is queue 1; controlq is queue 0 (to-verify-at-impl — research/ is empty, see plan M7).
- Compositor cursor path (host output side, reference only): `cells/drivers/virtio-gpu/src/main.rs` (CUR_SET/CUR_MOVE), compositor `render.rs`.
- GET_DISPLAY_INFO already lands in phase 01/02: `virtio_gpu/command.rs` (do NOT re-implement here, M6).

## Overview
- **Priority:** P2 (completes Track A scope; not required for first pixels)
- **Status:** complete (implementation; interactive cursor proof remains in phase 05)
- **Description:** Implement the cursorq (UPDATE_CURSOR / MOVE_CURSOR) so the guest can show a
  pointer. Wire the cursor to the compositor's existing cursor mechanism where feasible; otherwise
  fold the cursor into the scanout blend. **GET_DISPLAY_INFO is NOT in this phase** — the real
  280 B response moved to phase 01/02 (M6), because the driver's init-time probe consumes it long
  before any cursor exists.

## Key Insights (to-verify-at-impl — research/ is empty, plan M7)
- **Cursorq (queue 1) commands are pure fire-and-forget** — `virtqueue_add_sgs(outcnt=1, incnt=0)`,
  NO response descriptor. The device must still push the used buffer (frees the ring slot / fires
  `virtio_gpu_cursor_ack`) but writes **zero** response bytes. `process_notify` already pushes a
  used entry per chain; return `0` from the handler for cursor commands.
- `UPDATE_CURSOR=0x0300`, `MOVE_CURSOR=0x0301`, same struct
  `virtio_gpu_update_cursor { hdr; virtio_gpu_cursor_pos pos; u32 resource_id; u32 hot_x; u32 hot_y; u32 pad }`,
  `cursor_pos { u32 scanout_id; u32 x; u32 y; u32 pad }`. MOVE uses only `pos`.
- Cursor resource is a normal 2D resource (typically 64×64) created via RESOURCE_CREATE_2D +
  ATTACH_BACKING + TRANSFER_TO_HOST_2D, then referenced by UPDATE_CURSOR.resource_id — so cursor
  pixels already flow through the phase-02 resource table (as the small bounded cursor host buffer,
  within the byte budget); only position + overlay is new.

## Requirements
**Functional**
1. UPDATE_CURSOR: record cursor resource_id + hotspot + position; MOVE_CURSOR: update position only.
2. Cursor visibly tracks guest pointer in the compositor output.
3. Cursorq handler returns 0 bytes (no response), still frees ring slots.

**Non-functional**
- Cursor logic in `virtio_gpu/cursor.rs` (< 200 LOC). Prefer reusing the compositor's cursor
  opcodes over CPU-compositing the cursor into the scanout Grant, IF the VMM can drive them; the
  host virtio-gpu Driver Cell owns hardware cursor (`cells/drivers/virtio-gpu`), but the compositor
  is the client-facing endpoint — confirm during impl whether a client cell can request a cursor
  overlay, else composite the cursor into the scanout Grant (simpler, always works).

## Architecture
Two viable cursor renderings (pick during impl, default = fallback):
- **A. Compositor cursor overlay** (if the compositor exposes a client cursor op): forward cursor
  resource + pos to the compositor; hardware/overlay cursor, no per-move full copy.
- **B. Fallback — composite into scanout Grant:** on MOVE/UPDATE, re-blend the cursor resource's
  pixels at `pos - hotspot` into the scanout Grant and DamageNotify the affected rect(s). Always
  works with the existing compositor (no new opcode), costs a small extra copy per move. **Origin
  arithmetic is signed and bounds-checked** (see Security, C7b) — `pos - hotspot` underflows on u32.

## Related Code Files
- **Create:** `cells/services/hypervisor/src/virtio_gpu/cursor.rs`.
- **Modify:** `cells/services/hypervisor/src/virtio_gpu.rs` (cursorq dispatch → cursor.rs),
  `virtio_gpu/command.rs` (cursor struct codecs only — display_info encoder already exists from
  phase 01/02, M6), `virtio_gpu/scanout.rs` (cursor overlay for fallback B).

## Implementation Steps
1. command.rs: cursor struct parse (the `resp_display_info` encoder already lives here from phase 01/02).
2. virtio_gpu.rs: route `notify(1,...)` to cursor.
3. cursor.rs: state (resource_id, hot_x/y, x, y); decide overlay-vs-composite; implement chosen path
   with signed, clipped origin arithmetic (C7b).
4. Boot Alpine + Xorg/Weston (SW) on the hv-arm-gui image; confirm pointer visible + tracking.

## Todo List
- [x] command.rs: cursor codec (display_info already done in phase 01/02)
- [x] cursorq dispatch (0-byte response, ring frees)
- [x] software cursor composite-into-Grant with signed/clipped origin (C7b)
- [ ] Alpine + Xorg SW: cursor visible + tracking

## Success Criteria
- A mouse pointer is visible and tracks in the compositor output on the hv-arm-gui image.
- No cursorq ring stall (guest `dmesg` clean); no VMM error logs on cursor commands.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Compositor has no client cursor op → path A impossible | M×L | Fallback B (composite into scanout Grant) always works; make B the default, A an optimization. |
| Cursor composite causes flicker (overwrites scanout pixels) | M×M | Keep a saved-under buffer for the cursor rect; restore before re-blit on move (with the same signed/clipped bounds, C7b). |
| `pos - hotspot` underflow (u32) → OOB write into the Grant (C7b) | M×H | Compute origin as signed `i64`; reject/clip origins `< 0` and `>= dims` before indexing; bound each cursor row by `grant_len`; same for the saved-under restore buffer. |

## Security Considerations
- Cursor `resource_id` validated against the resource table (`ERR_INVALID_RESOURCE_ID` on miss).
- **Cursor origin (C7b):** `pos - hotspot` is computed as signed `i64` (guest `hot_x`/`pos.x` are
  u32 and underflow on subtraction); reject or clip negative and `>= dims` origins BEFORE indexing;
  bound EACH cursor row by `grant_len`. The saved-under restore buffer uses the identical bounds.
- Cursor pixels already validated by the phase-02 transfer path (same checked-arithmetic clamps).

## Next Steps
Track A is scope-complete after this + phase 05. Phase 06 (x86) and phase 07 (Track B) are
independent.
