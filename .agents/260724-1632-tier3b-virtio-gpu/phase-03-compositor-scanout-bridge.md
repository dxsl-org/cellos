# Phase 03 — Compositor Scanout Bridge (the copy path)

## Context Links
- Plan: [plan.md](plan.md) · Prev: [phase-02](phase-02-resource-transfer-model.md)
- Compositor client contract: `libs/ostd/src/display.rs:59-201` (ViSurface create/attach/damage/drop).
- IPC opcodes + wire structs: `libs/api/src/services/display.rs:222-284` (compositor_ops, AttachGrant, DamageNotify, COMPOSITOR_ENDPOINT=5).
- Compositor ATTACH_GRANT / DAMAGE handling: `cells/services/compositor/src/main.rs:273-327` (ownership by sender TID at :286).
- Grant syscalls granted in phase 01: `libs/api/src/abi/syscall.rs:200-235`.

## Overview
- **Priority:** P1 — this is the "pixels appear on screen" milestone.
- **Status:** complete (implementation; pixel evidence remains in phase 05)
- **Description:** Stand up the VMM as a compositor client: `CREATE_SURFACE` + `ATTACH_GRANT` ONCE at
  VM bring-up (non-blocking, masked, validated — NOT via `ViSurface`), then implement `RESOURCE_FLUSH`
  as `DAMAGE_NOTIFY`-only (the scanout pixels already landed in the owned Grant during phase-02
  SET_SCANOUT/TRANSFER, C5). Drives the existing compositor path to `COMPOSITOR_ENDPOINT=5`, plus the
  VMM-side `NotifyOnExit` registration matching the phase-01 compositor cleanup (C6).

## Key Insights
- **The VMM must NOT reuse `ostd::display::ViSurface` verbatim (red-team C3/C4).** ViSurface does
  BLOCKING `sys_send`+`sys_recv(0)` (wildcard) for CREATE_SURFACE and ATTACH_GRANT
  (display.rs:206-252). Inside the single-fiber MMIO-exit run loop — which also injects timers and
  polls net-RX (run_loop.rs:129-141) and has a 10 ms preempt budget — a blocking rendezvous send
  (task.rs:1098-1106) parks the whole VM; if the compositor wedges it parks forever. And a second
  wildcard `recv(0)` collides with the existing net req/reply consumer (net_backend.rs:30,50) →
  interleaved replies get parsed as a surface cap (documented cap-poisoning bug class in this
  project). The VMM reimplements the handshake, it does not call ViSurface.
- **Do the CREATE_SURFACE + ATTACH_GRANT handshake ONCE at VM bring-up** (before the run loop
  starts), or as a `sys_try_send`-driven state machine retried on later exits — never inside a FLUSH
  MMIO-exit handler. The reply is received with a **sender mask == comp_tid** (not `recv(0)`) via
  `sys_recv_timeout` (never unbounded); assert `sender == comp_tid` and validate the reply
  opcode/shape before trusting the returned cap. Invariant: the surface handshake and the net poll
  must not interleave, and no unsolicited inbound IPC is accepted while a handshake reply is awaited.
- The IPC SEQUENCE mirrors display.rs conceptually: `sys_grant_register(w*h*4)` →
  `sys_grant_share(reg_id, comp_tid, 0 /*RO*/)` → `sys_grant_slice` → CREATE_SURFACE → ATTACH_GRANT,
  reusing the wire structs (`AttachGrant`, `DamageNotify`, `compositor_ops`) from `api::display` — no
  new compositor opcode (locked decision) — but the send/recv mechanics are the non-blocking,
  masked, validated variant above.
- Compositor checks surface ownership by **sender TID** (main.rs:286): the VMM cell is the sender, so
  it owns the surface — clean, no impersonation risk.
- `sys_grant_share(perm=0)` = ReadOnly; the compositor gets read-only access and the kernel blocks
  any compositor write. Correct for a guest-fed buffer.
- **Resolution comes from the live compositor, not a constant.** The compositor sizes its framebuffer
  from `sys_get_resolution()` (compositor/src/main.rs:127) and there is NO `FALLBACK_WIDTH/HEIGHT`
  constant [STALE: earlier plan cited `display.rs FALLBACK_WIDTH/HEIGHT`, which does not exist].
  Verify the actual host resolution and reconcile the guest surface size against it (M8); 1024×768 is
  the assumed default to confirm, not a fixed truth.
- One host Grant + one surface for the single scanout. The single scanout copy already lands in the
  Grant during phase-02 SET_SCANOUT/TRANSFER (C5) → **FLUSH here is DamageNotify-only, no copy**.
- **Guest surface placement + focus (M8):** the VMM drives only CREATE/ATTACH/DAMAGE — it never sends
  MOVE_SURFACE/RAISE_SURFACE (display.rs:158,171), so without an explicit contract the guest surface
  lands at default (0,0)/z, competing with native Tier-1 surfaces (compositor composites by owner
  TID, main.rs:286; input focus routes guest-region clicks to the VMM cell via `hit_test`,
  input_handler.rs:198-206). Define an explicit position + z-order for the guest surface and specify
  focus-routing behavior for the guest region.

## Requirements
**Functional**
1. Create the compositor surface + attach the Grant ONCE, at VM bring-up (before the run loop), or as
   a `sys_try_send` state machine (C3). Resolve comp TID via `sys_lookup_service(service::COMPOSITOR)`
   (same lookup pattern as run_loop.rs:31 for NET); assert `comp_tid != 0`. Reply received via masked
   `sys_recv_timeout` (sender == comp_tid), opcode/shape validated (C4).
2. On FLUSH: pixels are already in the Grant (phase-02 copy). Re-derive the flush dims from the
   currently-bound scanout resource (no cached geometry, M1); clamp the damaged rect to the surface
   AND Grant dims (m4); send `DamageNotify{ cap, rect }` (24 B, api::display), fire-and-forget.
3. Return `OK_NODATA` on the controlq response descriptor AND push the used-ring entry + `inject_irq(19)`
   **independent of the compositor send** (M5) so the guest ring frees even if the compositor is slow.
4. Register kernel `NotifyOnExit` for the VMM cell (C6) so the compositor's owner-death cleanup
   (phase-01 prerequisite) fires on abnormal VMM death (OOM-kill/panic skip Shutdown teardown).
5. On graceful guest shutdown / VM teardown: `DETACH_GRANT` + `DESTROY_SURFACE` + `sys_grant_unregister`
   so the compositor stops reading freed memory. Teardown is idempotent (`ensure_surface` recreates
   only after an explicit teardown; on device-reset/UNREF-of-scanout release the old surface+Grant to
   avoid the per-reboot ~3 MiB leak, M1).

**Non-functional**
- New file `virtio_gpu/scanout.rs` (< 200 LOC): owns the Grant, surface cap, the handshake state
  machine, and DamageNotify. A single grant helper hard-codes `perm=ReadOnly` and
  `target=comp_tid` (asserted `!= 0`) so no future call site can widen it; use `checked_mul` for the
  Grant size — do NOT inherit display.rs:70's unchecked `w*h*bpp` (m5).
- No `&mut [u8]` across an await (Law 2) — this path is non-blocking IPC; keep buffers owned.

## Architecture
```
VM bring-up (once, before run loop):
  scanout.rs: ensure_surface()
    → grant_register(w*h*4 via checked_mul) → grant_share(reg_id, comp_tid, RO) → grant_slice
    → sys_send(comp_tid, CREATE_SURFACE) → sys_recv_timeout(mask=comp_tid) → validate → cap
    → sys_send(comp_tid, ATTACH_GRANT)   → sys_recv_timeout(mask=comp_tid) → validate 0x01
  register kernel NotifyOnExit(self)                                              (C6)

guest RESOURCE_FLUSH(rect, res_id):
  → resource.rs: re-derive dims from the currently-bound scanout resource (no cache, M1);
                 no valid binding → ERR_INVALID_RESOURCE_ID
  → clamp rect to surface AND Grant dims (m4)
  → sys_send(comp_tid, DamageNotify{cap,rect}.encode())     [fire-and-forget]   (M5: after ring free)
  → push used-ring entry + inject_irq(19) INDEPENDENT of the send                (M5)
  → write OK_NODATA response on controlq desc
```
The single guest→host copy happens at phase-02 SET_SCANOUT/TRANSFER (guest backing → Grant, C5);
FLUSH here does NOT copy — the Grant already holds the pixels, FLUSH only notifies the compositor.
The compositor only ever reads a normal RO Cell Grant and stays `forbid(unsafe)`.

## Related Code Files
- **Create:** `cells/services/hypervisor/src/virtio_gpu/scanout.rs`.
- **Modify:** `cells/services/hypervisor/src/virtio_gpu.rs` (FLUSH → scanout bridge, notify-only),
  `cells/services/hypervisor/src/virtio_gpu/resource.rs` (expose the owned Grant + bound-resource
  dims; the guest→Grant copy itself lives here from phase 02),
  `cells/services/hypervisor/src/run_loop.rs` (resolve compositor TID + drive the bring-up handshake
  before the run loop; register NotifyOnExit).

## Implementation Steps
1. run_loop.rs: `let comp_tid = sys_lookup_service(service::COMPOSITOR).unwrap_or(0);` pass to
   `GpuDev::new(comp_tid)`. If 0, retry the lookup on later exits (state machine); GPU still answers
   controlq. Register kernel `NotifyOnExit` for the VMM cell (C6).
2. scanout.rs: `Scanout { comp_tid, cap, reg_id, ptr, w, h, state }`; `ensure_surface()` runs the
   register/share/slice/CREATE/ATTACH sequence ONCE, non-blocking, masked `sys_recv_timeout`
   (sender == comp_tid), reply validated (C4); `notify_damage(rect)` clamps (m4) + fire-and-forget
   DamageNotify; `teardown()` idempotent (release surface + Grant on reset/UNREF/drop).
3. virtio_gpu.rs: FLUSH case → re-derive dims from bound resource, `scanout.notify_damage(rect)`,
   push used-ring + inject_irq(19) independent of the send, then OK_NODATA.
4. Wire teardown into the run-loop shutdown path (RunOutcome::Shutdown) AND the compositor-side
   NotifyOnExit path (phase-01 prerequisite) for abnormal death.
5. Boot the **hv-arm-gui image (phase 00)**; write a known test pattern from the guest (e.g.
   `/dev/fb0` fill or a DRM dumb-buffer test) and confirm it appears in the Cellos compositor output
   (QEMU SDL/VNC or captured FB).

## Todo List
- [x] run_loop.rs: resolve + reconnect compositor TID; register NotifyOnExit(self) (C6)
- [x] scanout.rs: bounded handshake, clamped DamageNotify, retryable idempotent teardown
- [x] virtio_gpu.rs FLUSH → notify-only bridge; used-ring + inject_irq(19) independent of send; OK_NODATA
- [x] teardown on VM shutdown AND compositor NotifyOnExit cleanup on abnormal death
- [x] guest surface uses compositor default origin/z-order and normal input focus routing
- [ ] hv-arm-gui: guest test pattern visible in compositor

## Success Criteria
- On the hv-arm-gui image (phase 00), a guest-drawn pattern (fbcon text or a solid-color DRM dumb
  buffer) is visible in the Cellos compositor output, updating on guest redraw.
- Handshake completes without parking the run loop (timers keep injecting, net-RX keeps polling
  during and after); no cap-poisoning (net replies never mis-parsed as a surface cap).
- Compositor accepts ATTACH_GRANT (`\x01`), not `\x00`; surface owner check passes.
- Grant + surface released on graceful guest exit AND on abnormal VMM death via NotifyOnExit (no
  compositor read of freed memory; verify via compositor logs / no fault).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Blocking handshake IPC parks the single run-loop fiber → whole VM freezes (C3) | H×H | Handshake ONCE at bring-up (or `try_send` state machine); never inside a FLUSH exit; masked `sys_recv_timeout`, never unbounded `recv(0)`. |
| Second wildcard `recv(0)` collides with net req/reply → cap poisoning (C4) | H×H | Masked recv (sender == comp_tid); validate reply opcode/shape; surface handshake and net poll never interleave. |
| Compositor not yet registered when GPU probes | M×M | `unwrap_or(0)` guard; retry lookup on later exits until TID resolves; controlq still answers meanwhile. |
| Guest resolution ≠ host surface → clipped/garbled | M×M | Verify actual dims via `sys_get_resolution()` (not a constant); reconcile guest surface size (M8); clamp DamageNotify rect (m4). |
| DamageNotify coupled to ring-free → compositor throttle stalls the guest ring (M5) | M×H | Push used-ring entry + `inject_irq(19)` INDEPENDENT of the compositor send; cap bytes-per-flush. |
| Guest surface at default (0,0)/z, focus routing undefined (M8) | M×M | Define explicit position + z-order; specify focus-routing for the guest region (input_handler.rs:198-206). |
| Stale Grant painted after abnormal VMM death (C6) | M×H | Compositor NotifyOnExit cleanup (phase-01 prereq) + VMM NotifyOnExit registration here; teardown idempotent + released on reset/UNREF (M1). |

## Security Considerations (hostile-guest boundary #2 + the compositor client edge)
The single guest→host pixel copy is validated in phase 02 (into the Grant). This phase adds:
- **SET_SCANOUT geometry (C7a):** on SET_SCANOUT of a resource larger than the fixed Grant, require
  resource dims == Grant dims (else `ERR_INVALID_PARAMETER`) OR reallocate + re-ATTACH the Grant to
  match. In any copy into the Grant use INDEPENDENT source/destination strides and bound EACH ROW by
  both `grant_len` and grant width; NEVER derive the dst stride from the guest resource width.
- FLUSH `rect` re-derived from the bound resource and clamped to surface AND Grant dims (m4); no
  negative (u32 fields); no valid binding → `ERR_INVALID_RESOURCE_ID`.
- The Grant is shared **ReadOnly** to the compositor via the single grant helper (perm=RO,
  target=comp_tid asserted `!= 0`) — a compromised guest cannot write into the compositor, and no
  future call site can widen the perm/target (m5).
- FLUSH reads only the Grant (host-owned, validated), never guest memory directly — so a guest cannot
  race the backing pages during the compositor's read.
- **NotifyOnExit (C6):** the VMM registers kernel `NotifyOnExit` so the compositor drops its cached
  Grant pointer on abnormal VMM death; SAS frame-identity means a freed frame is reused, so a stale
  pointer would paint another cell's memory (LBI-boundary break).

## Next Steps
Phase 04 (cursor) and phase 05 (test matrix) build on this. Phase 03 completion (on the hv-arm-gui
image) = Track A functionally demonstrable.
