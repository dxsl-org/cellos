---
title: "Stack & Tile for Cell Tabs"
description: "Haiku-style stacking (many Cell Tabs in one Cell Frame) and tiling (frames glued by shared edges) in the compositor"
status: completed
priority: P2
effort: 10.5d
branch: feat/stack-and-tile
tags: [compositor, desktop, window-management, ux]
blockedBy: []
blocks: []
created: 2026-11-11
---

# Stack & Tile for Cell Tabs

## Overview
Makes the Haiku-style Cell Tab pay off (`research/cell-tab-value.md`). **Stack:**
several windows share one Cell Frame; tabs side by side, click selects. **Tile:**
frames glue along shared edges and move/resize/raise/hide together. User gestures
in the compositor (window menu first, then Win-key drags). No new ABI; existing
IPC view ops are hardened, not extended. Orientation: `reports/scout-report.md`.

## Key decisions
- **D1 Model.** Stack = existing `Frame`; tile group = new `TileGroup` of frames bound to shared x/y tabstops (hard constraints only). A frame is in ≤ 1 group.
- **D2 Authority.** User gestures may group windows of different apps. A frame with views of several owners is `Shared` (compositor-owned): frame-level IPC is refused on it; membership grants no pixels, input or Grants.
- **D3 Geometry/state.** The frame is the only geometry/state authority; a pure reconciler converges members one request at a time (configure is single-outstanding, no timeout); acks commit size only; hidden members are configured lazily on selection; members render and take input clipped to the frame rect.
- **D4 Gestures.** Menu first (Stack onto / Detach / Close all), then Win+drag tab (stack/detach), Win+drag near edge (tile), Shift+drag (slide tab). Meta key events always forwarded.
- **D5 Group ops.** Activation raises the group; move moves all; minimize hides all; Maximize on a tiled frame untiles it first, then maximizes; clients cannot minimize/maximize a member of a multi-view frame or a tiled frame; close is per tab, "Close all" asks one member at a time.

## Phases
| Phase | Name | Status | Checkpoint |
|-------|------|--------|------------|
| 1 | [Baseline and frame registry split](./phase-01-baseline-split.md) | completed | |
| 2 | [Every window in a frame; mixed-owner authority](./phase-02-frame-membership.md) | completed | |
| 3 | [Stacked tab strip, frame geometry and reconciliation](./phase-03-stacked-tabs.md) | completed | ✓ |
| 4 | [Stack, detach and close from the window menu](./phase-04-menu-stacking.md) | completed | ✓ |
| 5 | [Win-key drag to stack/detach, Shift-drag sliding tab](./phase-05-drag-gestures.md) | completed | ✓ |
| 6 | [Tile groups: tabstops, glue and group move](./phase-06-tile-groups.md) | completed | ✓ |
| 7 | [Shared-edge resize, close group and docs](./phase-07-tile-resize-ops.md) | completed | ✓ |

## Evidence
Verification and review: [reports/verification.md](./reports/verification.md); baseline: [reports/baseline.md](./reports/baseline.md).

## Dependencies
- Builds on the 2026-11-11 chrome work (icon+title tab, window menu, `SET_ICON`).
- Out of scope: taskbar/Spotlight grouping, persistence across reboot, app-registered sessions (Ocel/Chrome own tab strips), transactional group configure IPC (contracts.md:31 deviation logged in phase 7).

## Red Team Review
Session 2026-11-11 — 4 lenses (Security Adversary, Assumption Destroyer, Failure Mode Analyst, Dependency Trap Hunter), 24 raw findings → 13 after dedup, all accepted.

| # | Finding | Sev | Applied in |
|---|---------|-----|------------|
| C1 | Mixed-owner frames let the frame owner select/focus/move/flood another app's view over IPC; dead-tid owner | Critical | P2 `Shared` authority, refusals, owner re-derivation |
| C2 | Pending configure blocks state/close/configure; eager configure + silent owner wedges members | Critical | P3 reconciler, lazy hidden configure, resend on `DETACH_REPLACED_GRANT` |
| C3 | Hit-testing used member rects (input outside/under frame); client MOVE/RAISE/MINIMIZE bypass frames | Critical | P3 frame-rect input, padding swallowed, per-cap request rules |
| M1 | Ack commits stale absolute origin after frame move | Major | P3 size-only ack |
| M2 | Closing/minimized selected member hides whole frame; close-all undefined | Major | P3 reselection, P4 close queue |
| M3 | No invalidation on frame drop; stale menu/drag/tile targets | Major | P2 `FrameChange`, P4/P5 re-checks, P6 unglue hook |
| M4 | No regression gate before P3; GUI proof baseline unknown | Major | P1 baseline, P3/P6 checkpoints |
| M5 | Client sizes/minimums can force 256 MiB buffers or clamp panic | Major | P2 validation + caps, P7 empty-range |
| m1 | Swallowed Meta release; own modifier tracking | Minor | P5 `KeyEvent.modifiers`, always forward Meta |
| m2 | `frame.rs` 1141 lines grows; unreachable frame-limit path | Minor | P1 split, P2 const assert |
| m3 | "Stack onto" titles spoofable, covered frames listed | Minor | P4 icon+title, visibility rule; C1 makes lures harmless |
| m4 | Maximize state drifts per member | Minor | P3 frame state + `saved_rect` |
| m5 | Ghost/highlight not repainted on aborted drag | Minor | P5 stale marking |

## Validation
User decisions 2026-11-11:
1. S&T key: as Haiku — Win/Meta (Haiku's Option key); no Alt fallback.
2. Maximize on a tiled frame: untiles the frame, then maximizes it.
3. Group configure: commit immediately, members clip/pad until they catch up (contracts.md:31 deviation recorded in phase 7).
