---
phase: 4
title: "Probe, QEMU evidence, and documentation"
status: implemented
priority: P1
effort: 1d
dependencies: [1, 2, 3]
tier: medium
---

# Phase 04: Probe, QEMU evidence, and documentation

## Overview
The existing deterministic graphical probe is lifecycle-aware and the existing
`window-policy` RV64 QEMU scenario now exercises the complete user-visible
policy without weakening its raise/capture/background oracle. The source
scenario is the evidence artifact; this change does not execute validation
commands by instruction.

## Delivered Behavior
- Interactive probe roles set titles and pump `poll_events(8)` plus
  `poll_surface_events(8)` every tick.
- Configure handling calls `ViSurface::apply_configure`, which stages the
  replacement Grant, acknowledges the serial, swaps local dimensions, redraws,
  damages, and logs only after success. The silent role deliberately leaves a
  configure pending.
- The primary role delays then requests restore after a real minimized-state
  event. The close role rejects its first close request, accepts its second,
  logs both decisions, and destroys its surface after acceptance.
- The QEMU scenario drives frame/title/control pixels, titlebar drag, a
  lower-right resize, maximize/restore, minimize/restore, silent configure,
  and close rejection/acceptance via QMP tablet input and PPM scanout.
- Existing overlap, background, pointer capture, and selected keyboard-owner
  coverage remains in this test; separate compositor-cursor coverage is intact.

## Evidence Scope
The scenario waits for typed lifecycle log markers and samples the compositor's
actual scanout. It checks the client redraw at the resized extent, minimized
reblend, restored geometry, maximized work area, and accepted-close reblend.
No privileged forged-frame injection or probe-specific raw IPC is used.

## Related Files
- `cells/tests/window-policy-probe/src/main.rs` — lifecycle-aware roles and
  deterministic configure/close/state logs.
- `tests/integration/tests/window-policy.rs` — retained legacy oracle plus
  QMP/PPM policy scenario.
- `docs/system-architecture.md`, `docs/project-roadmap.md`, and
  `docs/project-changelog.md` — shipped policy and boundaries.
- `.agents/TODO.md` and this phase file — delivery/evidence status.

## Implementation Record
1. Added `wm-primary`, `wm-silent`, and `wm-close` alongside the legacy roles.
2. Kept one packaged probe and one existing integration target; no alternate
   launcher, raw IPC path, or test mock was added.
3. Adapted legacy fixed pixels from the removed focus border to the intentional
   compositor frame and active/inactive titlebar pixels.
4. Added QMP pointer actions, serial log waits, PPM assertions, and negative
   decoration-input/silent-configure assertions for each new transition.
5. Published only the implemented ownership, replacement-Grant, and desktop
   boundary contract.

## Task List
- [x] Add lifecycle-aware primary, silent, and close probe roles.
- [x] Extend the bounded `window-policy` QEMU scenario with PPM/log evidence.
- [x] Cover decoration, drag, resize, managed visibility, and close transitions.
- [x] Publish the final ownership, compatibility, and limitation contract.

## Evidence Status
- [x] Source evidence exists for the legacy background/capture/focus path and
  the new titlebar, drag, resize, minimize/restore, maximize/restore, silent,
  and close reject/accept paths.
- [ ] Runtime execution is intentionally not recorded here because the assigned
  change explicitly prohibits validation commands.

## Security Considerations
The probe must not expose raw privileged injection to production clients. Negative authorization/serial cases remain unit-level with controlled senders; QEMU uses only real input and owner API calls. Documentation must state that a close timeout does not authorize the compositor to reclaim a live owner's Grant.

## Risk Notes
QEMU timing cannot prove an exact wall-clock timeout without a bounded wait and a visible old-state assertion. PPM glyph samples must be tied to the selected deterministic compositor font before hard-coding coordinates. Do not let test-only logging or a probe-only raw IPC escape into `ViSurface` production API.

## Deviation Log
None.
