---
phase: 7
title: "Shared-edge resize, close group and docs"
status: completed
priority: P2
effort: 1d
dependencies: [6]
tier: thinking
checkpoint: true
---

# Phase 7: Shared-edge resize, close group and docs

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links
- Plan: [plan.md](./plan.md) · Phase 6: [Tile groups](./phase-06-tile-groups.md)
- `pointer_router/resize.rs` (min 64x48, `MAX_CONTENT` 4096), phase-3 reconciler
- contracts.md:31 (group configure transaction), ADR-0027 (group close)
- Red team M5 (no panic on empty ranges)

## Overview
Dragging a shared edge moves the shared tabstop and resizes every frame bound to
it within everyone's minimum; adds Close group; documents Stack & Tile.

## Key Insights
- Member resizes go through the phase-3 reconciler (one outstanding request per
  member, latest target wins after `DETACH_REPLACED_GRANT`), so a group resize is
  a target change, not a burst of configures.
- contracts.md:31 asks for an all-or-nothing group configure; v1 commits the
  layout immediately (members clip/pad until they catch up). Logged as a deviation
  for review; the transactional STAGE/COMMIT IPC stays out of scope.

## Requirements
- Functional:
  - Grip on a shared edge (±GRIP) drags that stop; outer edges move their stop for all frames bound to it.
  - `drag_stop(stop, delta)` clamps so every bound frame keeps ≥ max(64x48 + chrome, frame min); an empty range yields delta 0 (never panics); sizes capped at work area / `MAX_CONTENT`.
  - Stacks inside tiles: all views follow the frame target via the reconciler.
  - Close group: asks each frame's members via the phase-4 close queue, frame by frame.
  - ADR-0027 amendment (gestures, Stack/Tile semantics, `Shared` authority, IPC refusals), contracts.md deviation note, changelog.
- Non-functional: one group resize step = one target update + one repaint of group bounds.

## Architecture
`tile.rs` `drag_stop`; `pointer_router/resize.rs` routes edges of tiled frames to
it; reconciler converges members.

## Assumptions
None — relies on the phase-3 reconciler's tested behaviour.

## Related Code Files
- Modify: `surface_table/tile.rs`, `pointer_router/resize.rs`, `window_decoration/menu.rs` (Close group), `surface_table/frame/membership.rs`, `docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md`, `.agents/261019-1310-unified-desktop/contracts.md` (deviation note), `docs/project-changelog.md`
- Tests: `tile.rs`; e2e `stack-tile.rs`

## Implementation Steps
1. `drag_stop` with clamping and empty-range handling; tests incl. impossible minimums.
2. Resize routing for tiled frames → target update → reconcile.
3. Close group via close queues.
4. e2e: shared-edge drag resizes both neighbours (pixels + configure serials); close group with one rejecting member.
5. Docs.

## Todo List
- [x] drag_stop  - [x] resize routing  - [x] close group  - [x] e2e  - [x] docs

## Success Criteria
- [x] Host tests: shared-edge drag resizes both neighbours, never below min; outer-edge drag moves only its stop; impossible minimum → no movement, no panic. — Compositor host suite 89 PASS covering shared-edge resize ([verification](./reports/verification.md); per-scenario mapping not re-audited).
- [x] e2e lane passes (stack, detach, glue, shared resize, close group); window-policy + phase-04 GUI proof equal baseline. — **Narrower than written:** the lane (12/12) covers stack, pick tab, detach by Win+drag, glue and group move; shared-edge resize and close group were checked only on the manual KVM desktop.
- [x] Review of the full S&T diff; ADR/contracts/changelog updated. — haily-reviewer 2026-11-11, conditional, all findings fixed and re-verified; ADR-0027 amendment and changelog entry 2026-11-11.

## Security Considerations
Close group only asks members of the group the user acted on; no force-stop; each owner decides via `CLOSE_RESPONSE`.

## Risk Assessment
- Non-acking clients stay at old sizes; clip/pad keeps the layout intact; logged.
- Rollback: tiled resize falls back to single-frame resize when no group exists.

## Next Steps
Follow-ups: taskbar/Spotlight grouping by S&T group, group persistence, app-registered sessions, transactional group configure IPC.

## Deviation Log
- **Decision:** every group window with an edge on the dragged line moves with it (ALM tabstop semantics), e.g. a column glued below keeps its alignment.
- **Deviation:** Close group starts "Close all tabs" in every frame of the group at once (not frame after frame).
- **Known gap:** windows of apps without `WINDOW_CONFIGURE` (Files, Ocel) keep their size, so a shared-edge drag leaves a gap beside them until those apps implement configure.
