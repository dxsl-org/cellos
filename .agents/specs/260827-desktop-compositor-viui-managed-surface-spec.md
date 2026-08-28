# Spec: Desktop Compositor × ViUI Managed Surface Integration

**Date:** 2026-08-27
**Status:** Approved
**Version:** v1

## Context

The compositor already owns bounded window policy, decoration, lifecycle, and damage-clipped
composition, while the canonical ViUI demo remains compile-only and its framebuffer renderer
reports full-surface damage for every frame. This slice connects ViUI to one real managed surface
without adding a desktop shell, changing the display protocol, or widening a trust boundary.

## Functional Requirements

### Core Behaviors

- WHEN a managed ViUI application starts, the application SHALL create one interactive
  Grant-backed surface, set its bounded UTF-8 title, and render its ViUI content into that surface.
- WHEN a signal-only update produces a bounded dirty region, the renderer SHALL clip that region
  to the current surface and send an exact `DamageNotify` instead of full-surface damage.
- WHEN a structural or layout-affecting update occurs, the renderer SHALL perform a complete
  layout/repaint and report full-surface damage.
- WHEN the compositor sends `WindowConfigure`, the integration layer SHALL apply the replacement
  Grant transaction, acknowledge the matching serial, update renderer dimensions, relayout, and
  repaint the complete new surface.
- WHILE the surface is minimized, the integration layer SHALL suppress new presentation damage.
- WHEN a minimized surface is restored, the integration layer SHALL request one complete repaint.
- WHEN the compositor sends `WindowCloseRequest`, the integration layer SHALL expose the request
  to application policy and send an explicit accept or reject response.
- WHEN forwarded pointer or keyboard input targets the surface, the integration layer SHALL
  translate and dispatch it through the canonical ViUI v2 event path.

### Edge Cases & Failure Modes

- IF a dirty rectangle is empty or lies wholly outside the surface, THEN the renderer SHALL send no
  damage notification.
- IF a dirty rectangle crosses a surface boundary, THEN the renderer SHALL send only its clipped
  non-empty intersection.
- IF a configure event has the wrong capability, zero serial, invalid geometry, or stale serial,
  THEN the integration layer SHALL reject it without changing the active renderer dimensions.
- IF Grant staging or configure acknowledgement fails, THEN the integration layer SHALL preserve
  the last committed surface state and SHALL NOT free a Grant the compositor may still read.
- IF close policy rejects a close request, THEN the surface SHALL remain usable and repaintable.
- IF close policy accepts a close request, THEN the application SHALL destroy the surface and
  release its Grants before exiting the managed loop.
- IF lifecycle events for another surface are received, THEN the integration layer SHALL ignore
  them without mutating its own state.

### Out of Scope

- Taskbar, Start Menu, global window enumeration, or control of another Cell's surface.
- Multi-window ownership inside one application Cell.
- Window snapping, workspaces, persisted geometry/session state, or live stretched-resize preview.
- Client-side titlebar or resurrection of the legacy chrome behavior in `libs/viui/src/window.rs`.
- GLES2, GPU acceleration, or a new renderer backend.
- RPi3 HDMI, physical-board qualification, or display-driver work.
- New syscalls, opcodes, display messages, capability semantics, or changes under `libs/api/`.

## Acceptance Criteria

- [ ] AC-1: `viui-demo` creates one interactive surface, sets a title, and displays content generated
  from its checked-in `.vi` source. → test: QEMU ViUI managed-surface scenario
- [ ] AC-2: A signal-only widget update emits one clipped non-full `DamageNotify` matching the
  affected region. → test: focused `viui` renderer test plus QEMU marker
- [ ] AC-3: `None` damage or a layout-affecting update emits full-surface damage, while empty or
  off-surface damage emits no notification. → test: focused `viui` renderer tests
- [ ] AC-4: Resize, maximize, and restore commit only after replacement-Grant attachment and the
  matching configure acknowledgement succeed. → test: managed-surface lifecycle tests
- [ ] AC-5: A successful configure updates renderer size, runs layout for the new dimensions, and
  performs exactly one complete repaint. → test: ViUI integration test
- [ ] AC-6: Configure rejection or ambiguous IPC failure retains safe Grant ownership and leaves the
  previous committed surface usable or returns a fail-closed error. → test: failure-injection tests
- [ ] AC-7: Minimized state suppresses presentation damage; restore causes one complete repaint.
  → test: managed-surface state-transition tests
- [ ] AC-8: Close requests support observable accept and reject paths; accept cleans the surface and
  Grants, while reject preserves interaction. → test: managed-surface close-policy tests
- [ ] AC-9: Pointer and keyboard widget interaction still work after resize and restore, and existing
  compositor window-policy tests remain green. → test: QEMU managed-surface and window-policy scenarios
- [ ] AC-10: The final diff contains no change under `libs/api/` and introduces no syscall, opcode,
  or display wire message. → test: diff audit and existing API contract tests

## Constraints

- The compositor remains the sole owner of decoration, hit testing, focus, z-order, and managed
  window-state policy; ViUI owns only application content and its local widget state.
- Existing `ViSurface::apply_configure` ownership and ambiguous-failure guarantees remain
  load-bearing; the integration layer must not duplicate or weaken its Grant transaction.
- The implementation remains `no_std + alloc`, preserves `#![forbid(unsafe_code)]` for the demo,
  and adds no new unsafe block to ViUI.
- Existing headless `ViSurfaceRenderer`, legacy Elm APIs, and compositor-independent widget tests
  remain compatible.
- Code files remain below the project 200-line target through focused modules and composition.

## Open Questions

- None. Close acceptance is application policy and must be explicit; no default silent decision is
  part of this contract.

## Change Log

- v1 — 2026-08-27 — initial approved scope, AC-1 through AC-10.
