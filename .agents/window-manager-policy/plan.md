---
title: "Bounded compositor window-manager policy"
description: "Compositor titlebars and controls with capability-safe configure, close, and visibility lifecycles."
status: pending
priority: P1
effort: 4d
branch: main
tags: [display, compositor, abi, window-manager]
blockedBy: []
blocks: []
created: 2026-08-25
---

# Bounded compositor window-manager policy

## Context Links
- `7d5baabc` focus/raise policy; `826f32f9` compositor pointer routing.
- `.agents/desktop-decoration/` records the superseded focus-border baseline;
  Phase 3 replaces it with the compositor-owned frame/title/control layer.
- `libs/api/src/services/display.rs`, `libs/ostd/src/{display.rs,input.rs}`.
- `cells/services/compositor/src/{main.rs,surface_table.rs,pointer_router.rs,input_handler.rs,render.rs}` and `tests/integration/tests/window-policy.rs`.

## Overview
Add compositor-owned titlebars, drag, edge/corner resize, close, minimize, and maximize/restore for interactive `ViSurface`s. Client pixels remain an immutable-to-compositor Grant: resize is proposed first and becomes live only after the owner attaches a correctly sized replacement Grant and acknowledges that proposal.

## Key Insights
- Existing Grant attach changes the live dimensions immediately; this plan replaces that behavior only while a pending configure exists, preserving the legacy path otherwise.
- `ostd::input::poll_events` currently receives from the compositor and drops non-input frames, so typed lifecycle frames require one shared compositor-frame dispatcher with a bounded surface-event queue.
- Decorations must be painted after each interactive surface in z-order (not as a final global layer), clipped and damaged as part of the compositor framebuffer; backgrounds remain undecorated and noninteractive.

## Requirements
- Add fixed, `#[repr(C)]`, LE-encoded, size-asserted messages with `MAX_TITLE_BYTES = 64`; owner/cap/serial validation and existing opcode/API compatibility are mandatory.
- Preserve all existing input forwarding, capture, focus, legacy owned-pixel, Grant, background, cursor, and z-order behavior outside decoration hit zones.
- Reject stale, duplicate, malformed, wrong-sender, wrong-cap, and wrong-dimension messages without changing active geometry; all queues and titles are bounded.

## Architecture
`pointer → decoration hit-test → compositor transition → typed event → client event dispatcher → stage replacement Grant → ConfigureAck → atomic commit/damage`. Drag changes only compositor position; resize coalesces while one configure is outstanding. Close waits for an owner response; minimize hides from paint/hit-test; maximize/restore use the same configure handshake.

## Phases
| Phase | Name | Status |
|---|---|---|
| 1 | [Stable display ABI and client demultiplexing](./phase-01-stable-display-abi-and-demultiplexing.md) | pending |
| 2 | [Compositor lifecycle and staged configure state](./phase-02-compositor-lifecycle-and-staged-configure.md) | pending |
| 3 | [Decoration rendering and pointer policy](./phase-03-decoration-rendering-and-pointer-policy.md) | pending |
| 4 | [Probe, QEMU evidence, and documentation](./phase-04-probe-qemu-evidence-and-documentation.md) | implemented (runtime execution pending) |

## Dependencies
Phase 2 consumes Phase 1's frozen wire/API contract; Phase 3 consumes Phase 2 state transitions; Phase 4 validates all three. The existing desktop-decoration implementation is the visual baseline, not a runtime dependency.

## Compatibility and No-Regression
Old clients keep `CREATE_SURFACE`, legacy `WRITE_PIXELS`, normal `ATTACH_GRANT`, and `poll_events` behavior. The window policy adds no taskbar, snapping, themes, persistence, keyboard move/resize, or client-side widgets. Every changed/new implementation file stays at or below 200 lines by extracting focused modules.

## Success Criteria
The RV64 QEMU probe demonstrates every specified transition while the original two-surface, capture, focus, background, cursor, and legacy Grant paths retain their observed behavior.

## Security Considerations
Only a kernel-authenticated owner can change its surface; serials, fixed layouts, checked geometry, bounded event storage, and staged read-only Grants make malformed or delayed frames fail closed.

## Risk Assessment
The principal hazards are dropped compositor events, premature Grant replacement, stale exterior pixels, and decoration input leaking to clients. The phases resolve them with a shared dispatcher, atomic stage/ACK commit, clipped full-frame damage, and mode-tagged compositor capture.

## Architectural Limit
There is intentionally no in-place resize and no live stretched preview: before ConfigureAck the old content/frame stays active; during a pending resize only the latest pointer geometry is retained. A minimized window has no compositor restore affordance because taskbar work is excluded; its owner restores it through the additive API.
