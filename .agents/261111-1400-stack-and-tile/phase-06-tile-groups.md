---
phase: 6
title: "Tile groups: tabstops, glue and group move"
status: completed
priority: P2
effort: 1.5d
dependencies: [5]
tier: thinking
checkpoint: true
---

# Phase 6: Tile groups: tabstops, glue and group move

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links
- Plan: [plan.md](./plan.md) · Phase 5: [Drag gestures](./phase-05-drag-gestures.md)
- Zeidler et al. 2013 §2 (tabstops/areas, hard min-size constraints) — research/cell-tab-value.md
- Red team M3 (tile cleanup), M4 (e2e per slice)

## Overview
Frames glue edge to edge into a tile group that moves, raises and hides as one.
Builds the model and glue/unglue; shared-edge resize is phase 7.

## Key Insights
- Haiku tiles via shared tabstops; hard constraints (min sizes, screen) suffice for v1.
- Tile groups contain frames (stacks), so Stack and Tile compose.
- Every frame drop must unglue first (phase 2 `remove_view`/`FrameChange` is the hook).

## Requirements
- Functional:
  - `TileGroup { id, x_stops, y_stops, members: Vec<(FrameId, Edges)> }`; frame rects of members derived from stops only.
  - Glue: Meta+drag a frame so an edge comes within 16px of another frame's parallel edge with ≥ 50% overlap; both edges highlight; release snaps flush and binds the shared stop; gluing to a grouped frame joins that group (groups merge).
  - Unglue: menu "Untile"; Meta+drag out; frame dropped (owner death, stacked into another frame, destroyed) → unglued via the `FrameChange` hook; groups split into connected components.
  - Group move: dragging any member's tab moves the group; the capture is keyed by group and re-checked on each move (member death updates the moved set).
  - Group raise/minimize: activating a member raises all members (focused last); menu Minimize hides the group (frame states via reconciler); restoring any member's frame restores the group. Client per-cap MINIMIZE/MAXIMIZE on tiled frames refused (as for stacks).
  - Maximize (menu or double-click) on a tiled frame untiles it (group split as for Untile), then maximizes it; Restore returns it to its pre-maximize rect untiled (user decision 2026-11-11).
- Non-functional: ≤ 16 frames/group, ≤ 32 groups; beyond → refused glue, logged.

## Architecture
`surface_table/tile.rs` (`TileRegistry`) next to `FrameRegistry`, threaded with it;
pure geometry, host-testable. Glue gesture in `pointer_router/tile_drag.rs`.

## Assumptions
- **Claim:** visible-frame edges (body + tab strip) are the glue line; vertical glue keeps the lower frame's tab visible.
  **Confidence:** medium. **How to verify:** host render preview.

## Related Code Files
- Create: `surface_table/tile.rs`, `pointer_router/tile_drag.rs`
- Modify: `surface_table/frame/{membership,geometry}.rs`, `pointer_router/input.rs`, `window_decoration/menu.rs` (Untile), `render.rs` (edge highlight), `z_order.rs`, `main.rs` (thread registry)
- Tests: `tile.rs` tests; e2e `stack-tile.rs`

## Implementation Steps
1. `tile.rs` model: glue/merge, unglue/split, move group, rect derivation; tests.
2. Hook `FrameChange` → unglue.
3. Glue detection + highlight + drop; Untile.
4. Group move/raise/minimize; Maximize untiles then maximizes; refusals for client state requests.
5. e2e: glue two windows side by side, drag one tab → both move; Minimize hides both.

## Todo List
- [x] model  - [x] drop hook  - [x] glue gesture  - [x] group ops  - [x] e2e

## Success Criteria
Evidence for this phase: [verification](./reports/verification.md) (compositor host suite 89 PASS covering tile groups; per-scenario mapping not re-audited).
- [x] Host tests: A|B shares one x stop; C under B joins; untiling B splits {A},{C}; middle frame death splits the group; stacking a tiled frame unglues it; group drag survives a member's death; 17th frame refused.
- [x] Host test: raising a member puts every group member above non-members, focused on top.
- [x] Host test: Maximize on the middle frame of A|B|C splits {A},{C}, maximizes B; Restore returns B untiled to its prior rect. — `maximizing_the_middle_of_a_tile_group_untiles_it` (`pointer_router/gesture_tests.rs`).
- [x] e2e (glue + group move + minimize) + window-policy + phase-04 GUI proof equal baseline. — Stack-tile lane 12/12 covers glue and group move; group minimize was checked only on the manual KVM desktop.

## Security Considerations
Grouping is presentation only; members keep separate focus, input and Grants;
no client can minimize or maximize another owner's frame through a group.

## Risk Assessment
- Stops vs surface rects drift → member rects derived from stops only; ack commits size only (phase 3).
- Rollback: tile registry empty → phase-5 behaviour.

## Next Steps
Phase 7: shared-edge resize and remaining group ops.

## Deviation Log
- **Deviation:** no stored tabstops. The registry keeps joins `(a, b, Right|Below)`; shared lines are read off the windows' geometry, so they cannot drift. Group = connected component of live joins; a dropped frame leaves its group in `drop_frame`.
- **Decision:** an app's own MINIMIZE on a tiled window minimizes only itself; menu Minimize hides the group and marks it; restoring a marked member brings back only the marked windows. An app's own MAXIMIZE on a tiled window is refused; its own MOVE/SNAP/FULLSCREEN untiles it.
- **Review fix:** maximizing a tiled window (menu or double click) now untiles it first (`actions::apply`; host test `maximizing_the_middle_of_a_tile_group_untiles_it`).
