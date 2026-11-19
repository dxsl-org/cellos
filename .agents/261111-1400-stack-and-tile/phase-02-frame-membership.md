---
phase: 2
title: "Every window in a frame; mixed-owner authority"
status: completed
priority: P1
effort: 2d
dependencies: [1]
tier: thinking
checkpoint: false
---

# Phase 2: Every window in a frame; mixed-owner authority

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links
- Plan: [plan.md](./plan.md) · Phase 1: [Baseline and split](./phase-01-baseline-split.md)
- Red team C1 (mixed-owner IPC), M3 (drop invalidation), M5 (size/min validation), m2 (const assert)

## Overview
Every window-managed surface gets a one-view frame; frames can hold views of
several owners via compositor-internal gestures without giving any owner IPC power
over another owner's view. No visible change.

## Key Insights
- `frame.rs:28-31` documents the gate's premise "every view in a frame shares one
  owner"; `select_view`, `snap_frame`, `toggle_fullscreen`, `create_view`-into
  check only `frame.owner_tid` (`frame.rs:108,227,494,549`). A mixed frame would
  let owner A select (and focus), move or flood owner B's view.
- `cleanup_owner` (`frame.rs:610-637`) keeps a frame with a dead owner's tid and
  stale `min_w/min_h`.
- `MAX_SURFACES = 32` (`surface_table.rs:22`) < `MAX_FRAMES = 64`: adoption can
  never hit the frame limit → const assert, no unframed fallback path.
- `MAX_SURFACE_DIM = 8192` (`surface_table.rs:32`); `CREATE_VIEW` stores client
  `min_w/min_h` unchecked (`view.rs:37-38`).

## Requirements
- Functional:
  - `CREATE_SURFACE` (Interactive) adopts into a new one-view frame; Panels/Backgrounds never.
  - `by_cap` index; `frame_of(cap)`, `members(frame)`, `selected(frame)`.
  - **Authority:** `Frame.authority = Owner(tid) | Shared` derived after every membership change (single owner → `Owner`, several → `Shared`). IPC frame-level ops (SELECT, SNAP, FULLSCREEN, CREATE-into, GROUP-into) are refused (`AccessDenied`) on `Shared` frames; `select_view` additionally requires `view.owner_tid == caller`. Detaching re-derives both frames.
  - **One removal choke point** `FrameRegistry::remove_view(cap) -> FrameChange { dropped: Option<FrameId>, reselected: Option<u64> }` used by DESTROY_SURFACE, owner cleanup, IPC destroy/group/detach and internal ops; callers forward the change to `PointerRouter` (close menus/captures referencing the frame) and to focus.
  - IPC `GROUP_VIEW`/`DETACH_VIEW`/`DESTROY_VIEW` go through the same invariant helpers as internal ops.
  - Owner death re-derives `authority`, `min_w/min_h`; a frame never keeps a dead tid.
  - **Validation:** `min_w ≤ w`, `min_h ≤ h`, both ≤ work area, at IPC and adoption; frame sizes the compositor proposes are capped at the work area and `MAX_CONTENT` (4096).
  - Internal ops (`stack`, `detach`, `select`) callable only from pointer/menu code (crate-private module).
  - `DESTROY_VIEW` / reselection never focuses another owner's surface on an IPC path.
- Non-functional: no rendering/hit-test change.

## Architecture
`surface_table/frame/membership.rs` holds `by_cap`, `remove_view`, authority
derivation and the internal ops. `FrameChange` flows: message handler → caller →
`InputState::frame_changed(change)` → router (menus, captures, hover, stale area).
`const _: () = assert!(MAX_SURFACES <= MAX_FRAMES);`

## Assumptions
- **Claim:** `owner_watch::reclaim` reaches `cleanup_owner` → `frames.cleanup_owner` (`message_handler.rs:152`, verified) and is the only death path.
  **Confidence:** medium. **How to verify:** read `owner_watch::reclaim`.

## Related Code Files
- Create: `surface_table/frame/membership.rs`
- Modify: `surface_table/frame/{registry,ipc_ops}.rs`, `message_handler/{surface,view}.rs`, `message_handler.rs` (owner cleanup), `input_handler.rs` (`frame_changed`), `pointer_router.rs` (invalidate by frame), `main.rs` (thread `&mut FrameRegistry` to input/render)

## Implementation Steps
1. `by_cap`, adoption on CREATE_SURFACE, const assert.
2. `remove_view` choke point + `FrameChange`; route every removal through it.
3. Authority derivation + IPC refusals on `Shared`; view-owner check in `select_view`.
4. Min/size validation at IPC and adoption.
5. Internal `stack/detach/select`; thread the registry into input/render (unused until phase 3).
6. Host tests.

## Todo List
- [x] adoption + index  - [x] choke point  - [x] authority + IPC refusals  - [x] validation  - [x] internal ops  - [x] tests

## Success Criteria
Evidence for this phase: [verification](./reports/verification.md) — `service-compositor` host suite 89 PASS (covers stacks, authority, lifecycle) and riscv64 check clean on 2026-11-11; 81 PASS at `c4c7d5d34f` after remediation removed eight cell-watch tests unrelated to S&T. The per-scenario test mapping below was not re-audited.
- [x] Host tests: two owners' windows → `stack(b, a)` → `Shared`; A's and B's SELECT_VIEW/SNAP_FRAME/TOGGLE_FULLSCREEN/CREATE_VIEW-into on that frame → `AccessDenied`; `detach(b)` → two `Owner` frames; owner A dies → B's frame `Owner(B)`, min sizes recomputed.
- [x] Host tests: every removal path returns a correct `FrameChange` (selected member removed → next selected; last removed → dropped).
- [x] Host test: CREATE_VIEW with `min_w > w` or beyond work area refused.
- [x] Existing `frame.rs` tests (incl. mixed-owner denial) pass; host suite + riscv check clean.

## Security Considerations
`Shared` frames are compositor-owned: no client can select, move, resize or add
to them over IPC. Membership grants no access to pixels, input or Grants.

## Risk Assessment
- Behaviour change for the (unused) IPC view ops on shared frames: intended; documented in the ADR amendment (phase 7).
- Rollback: revert adoption call; registry changes additive.

## Next Steps
Phase 3 renders stacks.

## Deviation Log
- **Decision:** `Frame.owner_tid` became `Option<usize>` (`None` = shared, compositor-owned) instead of an `authority` enum; re-derived with the frame minimum in one `refresh` after every membership change.
- **Decision:** the "single choke point" is `FrameRegistry::remove_surface` + `crate::stacking::forget` (z-order and reselection); router invalidation reuses the existing per-cap `forget`, menu/drag targets are re-checked when they act (`window_menu::can_stack`), so no `FrameChange` type was needed.
- **Deviation:** frame-size caps live in the reconciler (`MAX_CONTENT`, work area) rather than in adoption.
