---
phase: 5
title: "Win-key drag to stack/detach, Shift-drag sliding tab"
status: completed
priority: P2
effort: 1.5d
dependencies: [4]
tier: medium
checkpoint: true
---

# Phase 5: Win-key drag to stack/detach, Shift-drag sliding tab

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links
- Plan: [plan.md](./plan.md) · Phase 4: [Menu stacking](./phase-04-menu-stacking.md)
- Haiku guide "Stack & Tile" and tab Shift+drag (research/cell-tab-value.md)
- Red team m1 (Meta release), M3 (stale ghost)

## Overview
Haiku's direct gestures: Win/Meta+drag a tab onto another frame's tab strip to
stack, out of its frame to detach; Shift+drag a single-tab window's tab to slide
it along the top edge.

## Key Insights
- `KeyEvent` already carries `modifiers` (`libs/api/src/services/input.rs:115`);
  the compositor sees every key event before forwarding (`input_handler.rs` Key
  path) → keep the last modifiers, no separate tracking or focus-loss reset.
- The layout maps scancodes 0x7D/0x7E to `Modifiers::META`
  (`cells/services/input/src/layout_us_qwerty.rs:273`).
- The Meta press reaches the app before a drag starts, so its release must reach it too.

## Requirements
- Functional:
  - Drag mode chosen at tab press from the last seen modifiers: Meta → `StackTab`, Shift (single-tab frame) → `SlideTab`, else `Move`.
  - Meta-drag: ghost outline follows; target = another frame's tab strip (x within strip, y within ±8px); target strip + dragged tab highlighted (accent 1px); release stacks; release elsewhere detaches a member of a multi-tab frame at the drop point, or moves a single-tab frame.
  - Esc during a Meta-drag cancels (frame returns to start); that Esc is consumed. Meta key events are never consumed (press and release always forwarded).
  - Target re-checked on every move and at drop; source or target vanishing cancels the drag and marks ghost/highlight area stale.
  - Shift-drag slides the tab: offset clamped to [0, frame_w − tab_w]; persists per frame; reset when the frame becomes a stack; double-click still toggles maximize.
- Non-functional: repaint only ghost/strip areas.

## Architecture
`InputState.modifiers` updated from each `KeyEvent`. `Capture::Drag` gets
`mode: DragMode`. `Frame.tab_offset` (single-tab) feeds `TabSlot`. Gesture code in
`pointer_router/stack_drag.rs`.

## Assumptions
- **Claim:** QEMU virtio-keyboard delivers KEY_LEFTMETA as 0x7D.
  **Confidence:** medium. **How to verify:** QMP `send-key meta_l`, serial log of the decoded key.
- **Claim:** host WM (WSLg/GTK) does not grab the Win key from the QEMU window.
  **Confidence:** low. **How to verify:** manual check; e2e uses QMP `send-key`, independent of the host WM. User decision: Meta only, as Haiku (no Alt fallback).

## Related Code Files
- Create: `pointer_router/stack_drag.rs`
- Modify: `input_handler.rs`, `pointer_router.rs`, `pointer_router/input.rs`, `window_decoration.rs` (tab offset, highlight), `window_decoration/raster.rs`, `surface_table/frame/membership.rs`, `render.rs`
- Tests: `pointer_router/tests.rs`; e2e `tests/integration/tests/stack-tile.rs`

## Implementation Steps
1. Keep `modifiers` from key events.
2. Drag modes; Meta-drag target detection, highlight, ghost; drop actions; Esc cancel.
3. Shift slide offset + clamp + reset.
4. e2e via QMP: Meta-drag stack/detach; Shift slide persists after an unrelated repaint.

## Todo List
- [x] modifiers  - [x] Meta-drag  - [x] Esc cancel  - [x] slide  - [x] e2e

## Success Criteria
Evidence for this phase: [verification](./reports/verification.md) (compositor host suite 89 PASS covering gestures; `service-input` 52 PASS covering modifier transitions; per-scenario mapping not re-audited).
- [x] Host tests: target detection (in strip, ±8px, not own frame); drop outcomes for 1- vs N-tab source; Esc restores start rect; target death mid-drag cancels and marks area stale; Meta press/release both forwarded (balanced).
- [x] Host test: slide offset clamps; reset on stacking.
- [x] e2e + window-policy + phase-04 GUI proof equal baseline. — Stack-tile lane 12/12 (includes Win+drag out and beside); window-policy at baseline step 3; GUI proof unchanged.

## Security Considerations
Only the cancelling Esc is consumed; apps keep balanced modifier key events.

## Risk Assessment
- Host grabbing the Win key in the QEMU window blocks the gesture there only; menu commands (phase 4) remain; documented.
- Rollback: gestures gated by modifiers; plain drag unchanged.

## Next Steps
Phase 6: tiling.

## Deviation Log
- **Surprise → deviation:** the assumption "the compositor sees every key event" was false: the input service swallows pure modifier keys. It now also sends Shift/Ctrl/Alt/Win transitions on its pointer channel (reaches only the compositor), which consumes them; apps are unchanged. QMP `meta_l` verified on KVM.
- **Decision:** Win+drag moves nothing until the drop (ghost outline + accent target), so detach and move happen once at release.
