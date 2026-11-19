---
phase: 3
title: "Stacked tab strip, frame geometry and reconciliation"
status: completed
priority: P1
effort: 2.5d
dependencies: [2]
tier: thinking
checkpoint: true
---

# Phase 3: Stacked tab strip, frame geometry and reconciliation

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links
- Plan: [plan.md](./plan.md) · Phase 2: [Frame membership](./phase-02-frame-membership.md)
- `surface_table/configure.rs:23-157`, `surface_table/lifecycle.rs:13-68`, `pointer_router/{input,target}.rs`, `render.rs`
- Red team C2 (pending configure), C3 (input/IPC writers), M1 (ack origin), M2 (closing member), Minor (maximize drift)

## Overview
Frames with N views draw N tabs on one body; only the selected view paints and
takes input; the frame owns geometry and state, and a reconciler converges every
member to it despite one-at-a-time configure and silent owners. This rewrites the
paint/hit/focus path for every window, so it is a checkpoint with both QEMU lanes.

## Key Insights
- `request_state`, `begin_close`, `begin_configure` all refuse while a configure is
  pending (`configure.rs:31-36,66-73`, `lifecycle.rs:17-21`); nothing times out;
  after an ack `retired_grant_id` blocks the next configure until the client sends
  `DETACH_REPLACED_GRANT` (`grant.rs:71-85`).
- An ack commits `pending.rect` including its origin (`configure.rs:134-147`).
- `begin_close` → `Closing` (not visible-for-paint); an accepted close stays
  `Closing` until the owner destroys (`lifecycle.rs:28,53-68`).

## Requirements
- Functional:
  - **Frame target:** `Frame { rect, state: Normal|Maximized|Minimized, saved_rect }` is the only geometry/state authority for members of multi-view frames.
  - **Reconciler** (`frame/geometry.rs`): after any frame change, gesture, ack, `DETACH_REPLACED_GRANT` or close response, for each member with nothing pending: if its state/size differs from the frame target, issue the one request that moves it closer (configure Resize/Maximize, MINIMIZE, RESTORE). Hidden members are reconciled only when selected or when the frame state changes (lazy, contracts.md:31). Never more than one outstanding request per member.
  - **Ack origin:** for framed members an ack commits size only; x/y always come from the frame rect.
  - **Paint:** chrome painted per frame (independent of the selected member's state); N tab slots `min(tab_width, (body_w - gaps)/N)` wide; tab narrower than icon + 24px shows icon only; selected member's buffer blitted clipped to the frame content rect, remainder filled with palette `bg`.
  - **Input:** hit-test, hover, grips and pointer origin for framed surfaces use the frame rect; pointer events inside the frame but outside the member's real buffer are swallowed; hidden members get no pointer/key input; `Hit::Tab(i)`; icon (menu) only on the selected tab.
  - **Selection:** click a non-selected tab → select, raise group, focus its owner (user gesture only). When the selected member becomes `Closing`/`Minimized`/destroyed, select the next live member and move focus there; a rejected close keeps the current selection.
  - **Client per-cap requests on framed members:** MOVE_SURFACE moves the frame (and all members); RAISE_SURFACE raises the group without selecting/focusing a hidden member; MINIMIZE/MAXIMIZE/RESTORE from a member of a multi-view frame are refused (single-view frames unchanged).
  - **Group ops:** drag moves the frame; resize sets the frame rect (capped at work area/MAX_CONTENT) and reconciles; maximize/restore and double-click toggle the frame state (`frame.saved_rect`); z-order keeps members contiguous (`z_order::raise_all`).
- Non-functional: single-view frames behave and render exactly as today.

## Architecture
`render.rs` iterates frames (selected member) in z-order; `window_decoration`
geometry takes `TabSlot { index, count }`; `Style` gets `[TabInfo; ≤32]`
(icon, title, selected). `pointer_router/target.rs` resolves frames first, then
the selected member. Reconciler is pure over (frame target, member states) → list
of requests; I/O done by the caller.

## Assumptions
- **Claim:** 13px Inter titles stay readable at tab width ≥ 60px.
  **Confidence:** medium. **How to verify:** host render preview with 2/3/5 tabs.

## Related Code Files
- Create: `surface_table/frame/geometry.rs` (target + reconciler), `pointer_router/stack_hit.rs` if `input.rs` exceeds 300 lines
- Modify: `window_decoration.rs`, `window_decoration/raster.rs`, `render.rs`, `framebuffer.rs` (clip/pad), `pointer_router/{input,target}.rs`, `input_handler.rs` (focus, swallow), `surface_table/configure.rs` (ack size-only for framed), `message_handler/{surface,window,grant}.rs` (per-cap writers, reconcile hooks), `z_order.rs` (`raise_all`)
- Tests: `window_decoration/tests.rs`, `pointer_router/tests.rs`, `surface_table/frame/tests.rs`

## Implementation Steps
1. TabSlot geometry + N-tab raster + per-frame chrome.
2. Frame target + pure reconciler with tests (incl. never-acking member).
3. Ack size-only for framed members; reconcile hooks on ack/detach-replaced/close response.
4. Input: frame-rect hit-testing/origin, swallow padding, hidden members excluded, `Hit::Tab`.
5. Selection rules (click, closing/minimized reselection, rejected close).
6. Per-cap client requests on framed members.
7. Frame-wide drag/resize/maximize/double-click; `raise_all`.
8. QEMU: run window-policy + phase-04 GUI proof (single-view regression) and compare with `reports/baseline.md`.

## Todo List
- [x] tabs/chrome  - [x] reconciler  - [x] ack size-only  - [x] input routing  - [x] selection  - [x] client requests  - [x] group ops  - [x] QEMU lanes

## Success Criteria
Evidence for this phase: [verification](./reports/verification.md) — `service-compositor` host suite 89 PASS on 2026-11-11 (81 at `c4c7d5d34f`, eight unrelated cell-watch tests removed since). The per-scenario test mapping below was not re-audited.
- [x] Host tests (pure): reconciler converges a 3-member frame through resize→maximize→restore with acks in any order; a never-acking member blocks only itself; its pending never blocks the frame state change for others; latest target wins after `DETACH_REPLACED_GRANT`.
- [x] Host test: configure → frame drag → ack ⇒ member at the frame's origin.
- [x] Host tests: oversized and undersized selected member — no pointer event routed outside the frame rect; padding swallows; hidden member never targeted.
- [x] Host test: closing the selected tab of a 2-tab frame keeps the other tab painted and selected; rejected close keeps selection.
- [x] Host test: MINIMIZE/MAXIMIZE from a member of a 2-view frame refused; MOVE_SURFACE moves the frame. — Narrowed by the Deviation Log: refused on shared frames and for hidden tabs only.
- [x] QEMU: window-policy and phase-04 GUI proof results equal `reports/baseline.md`. — window-policy fails at the baseline step 3; GUI proof PASS, 293 checks, hashes unchanged (baseline run: 295).

## Security Considerations
No input reaches a hidden member or leaves the frame rect; focus changes to
another owner only on a user gesture.

## Risk Assessment
- Reconciler bugs could leave members at wrong sizes; mitigated by pure tests and clip/pad rendering.
- Rollback: revert the phase; phase 2 data model stays valid.

## Next Steps
Phase 4: menu commands create/dissolve stacks.

## Deviation Log
- **Deviation (D3):** no frame-owned rect. The shown window *is* the frame; hidden tabs sit at its origin (`stacking::sync`, the only writer of `behind_tab`) and are configured to its intended size/state (`stacking/reconcile.rs`). Acks re-sync origins instead of committing size-only, so `configure.rs` keeps its commit. Clip/pad is unnecessary: input and paint always use the shown window's own rect.
- **Decision:** a picked tab whose size differs is asked for the frame's size; the frame shows it at its own size until it answers (a silent app keeps its size).
- **Surprise → fix:** an app that never answers a configure (Files, Ocel ignore `WINDOW_CONFIGURE`) used to become unclosable/unminimizable; `begin_close` and `request_state(MINIMIZE/RESTORE)` no longer wait for a pending configure, which commits into the restore state when it lands (`surface_table/lifecycle_tests.rs`).
- **Deviation:** client MINIMIZE/MAXIMIZE/RESTORE-from-maximized are refused only on *shared* frames (several apps) and for hidden tabs; on a single-app stack the shown tab's own requests act on the frame. RESTORE from minimized is always allowed.
- **Surprise → fix:** content corner insets ignored the stack's square top-right corner (black notch); `content_row_insets` takes `Tabs`.
