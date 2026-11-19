---
phase: 4
title: "Stack, detach and close from the window menu"
status: completed
priority: P1
effort: 1.5d
dependencies: [3]
tier: medium
checkpoint: true
---

# Phase 4: Stack, detach and close from the window menu

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links
- Plan: [plan.md](./plan.md) · Phase 3: [Stacked tabs](./phase-03-stacked-tabs.md)
- `window_decoration/menu.rs`, `pointer_router/{input,control}.rs`, ADR-0027 "Labels, menus and new sessions"
- Red team M2 (close semantics), M3 (re-check targets), m3 (spoofable titles)

## Overview
Users create and dissolve stacks with explicit menu commands (ADR-0027's first
step). First end-to-end user-visible Stack feature, with its own QEMU lane.

## Key Insights
- `MENU_ITEMS` is fixed (`menu.rs:12`); `OpenMenu` is `Copy` → items in a fixed
  `[Option<MenuItem>; 12]` (no `heapless` in the compositor; only ostd has it,
  `libs/ostd/Cargo.toml:20`).
- Close state is per surface (`lifecycle.rs:13-61`), so several close requests
  can be in flight; an accepted close stays `Closing` until the owner destroys.

## Requirements
- Functional:
  - Sections: **Window** Maximize/Restore, Minimize · **Group** "Stack onto …" per candidate (icon + title of the candidate's selected tab), "Detach tab" when > 1 tab · **Close** "Close tab", "Close all tabs" when > 1.
  - Candidates: other frames not minimized, at least partly unobscured, whose bounds overlap or are within 48px; nearest first; ≤ 5.
  - **Action-time re-check:** target exists, is visible, is not the source; tab count still allows Detach; otherwise do nothing (menu closes).
  - Stack: source views appended to the target; source selected; source frame dropped via `remove_view`/`FrameChange`; reconciler sizes members to the target.
  - Detach: new frame at +24,+24 (clamped), current size, selected, raised; authority re-derived.
  - Close all: ask members one at a time in tab order, each selected (visible) while asked; an accepted-but-not-destroyed member is drawn dimmed, excluded from Stack/Detach and skipped; a member whose request could not be delivered stays and is reported in the log.
  - `forget`/`FrameChange` closes any open menu that references an affected frame.
- Non-functional: menu within the screen, ≤ 12 rows.

## Architecture
`MenuAction = Maximize | Minimize | StackOnto(FrameId) | Detach | CloseTab | CloseAll`;
`window_menu(frame, registry, table) -> [Option<MenuItem>; 12]`; dispatch in
`pointer_router/control.rs` → `frame/membership.rs` ops. Close-all progress lives
on the frame (`closing_queue`) and advances on each close response.

## Assumptions
None — no unverified claims (menu storage and close state verified above).

## Related Code Files
- Modify: `window_decoration/menu.rs`, `pointer_router.rs`, `pointer_router/{input,control}.rs`, `surface_table/frame/membership.rs`, `message_handler/window.rs` (close response advances queue), `render.rs`
- Create: `tests/integration/tests/stack-tile.rs` (+ `scripts/lanes.toml` row), probe roles in `cells/tests/window-policy-probe` if needed

## Implementation Steps
1. Dynamic menu model, geometry (variable rows/separators), paint with icons.
2. Candidate selection + action-time re-check.
3. Actions: stack, detach, close tab, close-all queue.
4. e2e lane: two probe windows → "Stack onto" → two tabs, one body; click tabs switches visible colour; "Detach tab" → two windows; "Close all tabs" with one rejecting and one silent member.

## Todo List
- [x] menu model + paint  - [x] candidates + re-check  - [x] actions + close queue  - [x] e2e lane

## Success Criteria
Evidence for this phase: [verification](./reports/verification.md) (host suite 89 PASS covering menu targets and close-all; per-scenario mapping not re-audited).
- [x] Host tests: menu rows for 1- vs 3-tab frames; candidates exclude minimized/fully covered/self, nearest first, ≤ 5; target dying while the menu is open → no-op.
- [x] Host tests: stack then detach restores the original members/authority; close-all with reject/silent members ends with exactly those members left.
- [x] e2e lane passes; window-policy + phase-04 GUI proof equal baseline. — `qemu-stack-tile-test.sh` 12/12; window-policy at baseline step 3; GUI proof 293 checks, hashes unchanged.
- [x] Review of the Stack slice diff. — Reviewed as part of the full S&T diff (haily-reviewer, 2026-11-11, findings fixed; verification report).

## Security Considerations
Rows show the candidate's icon and title as already drawn on its tab (both client
chosen); a lured stack grants nothing (phase 2 `Shared` authority).

## Risk Assessment
- Close-all across owners can leave a partial stack: intended; each owner decides.
- Rollback: menu items gated by frame count.

## Next Steps
Phase 5: direct-manipulation gestures.

## Deviation Log
- **Deviation:** section order is Window | Close | Group so Close stays the third row (the phase-04 GUI proof clicks it there; also steadier for the hand). First GUI-proof run failed on exactly this, fixed and re-run.
- **Decision:** "covered" means both the tab strip and the body lie under one window above (a peeking tab keeps a window offered).
- **Decision:** Close all asks one tab at a time and advances on each answer (`advance_close`), or when the asked tab disappears unanswered (`stacking::forget`); picking or closing a tab by hand stops it. A tab whose owner never answers and never exits stalls the rest (each owner decides).
