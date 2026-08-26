---
phase: 3
title: "Deterministic RV64 QEMU visual evidence"
status: complete
priority: P1
dependencies: [1, 2]
tier: medium
---

# Phase 03: Deterministic RV64 QEMU visual evidence

## Context Links
- `tests/integration/tests/window-policy.rs:1-121`
- `cells/tests/window-policy-probe/src/main.rs:16-120`
- `tests/integration/src/lib.rs` (`QemuRunner`, PPM helpers, QMP pointer helpers)
- `libs/ostd/src/display.rs:59-221`

## Overview
Extend the existing two-owner RV64 QEMU policy test with fixed, cursor-safe PPM pixels that prove focus-border appearance, replacement, old-border erasure, and unchanged client interiors. Reuse the real Grant-backed probe and QMP input path rather than introducing a synthetic renderer test.

## Key Insights
- The existing test boots a GPU/tablet/keyboard QEMU runner, launches `background`, `back`, and `front` as separate real cells, captures PPM output, and already proves raise/capture/focus behavior.
- Probe geometry is deterministic: back is `[80,240)×[80,240)`, front is `[160,320)×[120,280)`, and background is full-screen at the test resolution.
- Probe colors are BGRA: background renders RGB green, back renders RGB red, front renders RGB blue. The implementation must publish one fixed opaque focus-border BGRA/RGB pair for this test.

## Requirements
- Keep this single integration file at or below 200 lines; factor only a local pixel helper/constant if required.
- Before selection, assert a back-border exterior pixel is the background RGB color, not focus color.
- After clicking exposed back at `(100,100)`, wait for its existing press marker and assert a cursor-safe back exterior pixel (for example `(79,140)`) is focus color while an interior/back overlap sample stays red.
- Retain the established captured move/release, background exclusion, and keyboard-focus assertions unchanged.
- After those assertions, click front-only `(280,200)`, wait for front press/release, then assert old back exterior `(79,140)` restored green, new front exterior `(320,200)` is focus color, and front interior `(300,200)` stays blue.
- Use bounded existing `wait_for` transitions before every capture; do not rely on arbitrary sleep as the only ordering signal or add a new test cell unless the current probe cannot emit an essential marker.

## Architecture
The host test observes one real path: `window-policy-probe` writes immutable role colors into its own Grants; QMP click selects a cap; compositor reblends and draws scanout-only border; QMP `screendump` becomes a PPM sample. Coordinate choices are one pixel outside a surface and far from click/cursor positions, so they distinguish an exterior decoration from client content.

## Related Code Files
- Modify: `tests/integration/tests/window-policy.rs` — add named coordinates/colors and ordered PPM assertions; remain ≤200 lines.
- Reuse unchanged: `cells/tests/window-policy-probe/src/main.rs` — it already supplies two owners, fixed geometry/color, ready, press, release, move, and key markers.
- Reuse unchanged: `tests/integration/src/lib.rs` QEMU/QMP/PPM helpers, package/disk workflow, and existing prerequisite guard.
- No changes: production source beyond Phases 01–02, client display API, docs, CI, or test packaging unless an evidence gap is demonstrated.

## Implementation Steps
1. Define concise test constants for focus RGB, exterior/interior coordinates, and a helper that samples exactly one PPM pixel through `read_ppm_frame`/`pixel_region`.
2. After all three probe-ready markers, capture a baseline and assert the unselected back exterior is green; retain the current initial front-overlap assertion.
3. Click the exposed back surface, wait for its press marker, capture, and assert yellow (or the chosen fixed focus RGB) at `(79,140)` plus red at the existing back client/overlap sample.
4. Run the existing capture, background, and keyboard checks. Only after their negative-front assertions, click front-only, wait for front press and release, and capture the switched state.
5. Assert `(79,140)` is green again, `(320,200)` is focus RGB, and `(300,200)` remains front blue. The failure messages must name expected geometry/color and include `qemu.dump()` on wait failure.
6. Run the one named QEMU integration test through its established RV64 prerequisite workflow; record an explicit skip only when its existing guard cannot obtain kernel, disk, or QEMU.

## Todo List
- [ ] Add fixed exterior/interior PPM constants and a concise sampling helper.
- [ ] Assert no border before first interactive selection.
- [ ] Assert back selection paints exterior border without changing client samples.
- [ ] Preserve current capture/background/keyboard policy oracle.
- [ ] Assert front selection removes old border and paints new exterior border.
- [ ] Execute the named RV64 QEMU test with the existing prerequisite guard.

## Success Criteria
- [ ] The pre-click exterior is background green, the clicked-back exterior is the exact focus RGB, and back client pixels remain red.
- [ ] After front selection, the old back exterior is green, the front exterior is exact focus RGB, and front client pixels remain blue.
- [ ] Existing assertions still prove raise, capture, background exclusion, and keyboard routing in the same QEMU session.
- [ ] Any stale decoration, client-pixel overwrite, wrong stack order, or input-policy regression fails deterministically with a PPM/serial diagnostic.

## Risk Assessment
QMP/PPM evidence is asynchronous and cursor pixels can contaminate a sample. Use fixed coordinates outside every clicked cursor location, serial markers before each capture, and colors with no overlap. Avoid adding sleeps as synchronization; retain only the current short settling intervals where the pre-existing test needs them.

## Security Considerations
The test drives only ordinary QMP pointer/keyboard events against an existing local QEMU test VM. The guest probe uses public unprivileged surface/input APIs; it bypasses no compositor ownership check and exposes no new focus operation.

## Next Steps
Once this evidence passes, stop this slice. Any titlebar, metadata, drag/resize, close control, desktop shell, or taskbar work requires a separate approved plan.
