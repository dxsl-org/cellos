---
phase: 2
title: "Selected-cap invalidation and removal lifecycle"
status: complete
priority: P1
dependencies: [1]
tier: medium
---

# Phase 02: Selected-cap invalidation and removal lifecycle

## Context Links
- `cells/services/compositor/src/input_handler.rs:25-151`
- `cells/services/compositor/src/pointer_router.rs:19-151`
- `cells/services/compositor/src/main.rs:95-136,160-184,301-325`
- `cells/services/compositor/src/surface_table.rs:233-262`
- `cells/services/compositor/src/z_order.rs:26-45`

## Overview
Track the cap selected by an existing interactive left press, not merely its owner, so rendering can decorate it and reliably remove that decoration. Preserve the prior capture and keyboard-owner policy exactly.

## Key Insights
- `PointerRouter::route_button` already derives an interactive target from `hit_test`, sets capture and `focused_owner`, raises the cap, then invokes its activation callback.
- `hit_test` explicitly rejects `SurfaceRole::Background`; `RAISE_SURFACE` also rejects background surfaces in `main.rs:272-300`.
- Surface destruction and owner-exit cleanup already save a raw freed rectangle and schedule compositor dirty; the external border extends beyond that rectangle.

## Requirements
- Keep selected cap private in `PointerRouter` (for example, `Option<u64>`), while preserving `focused_owner` as the sole keyboard dispatch value and existing cap-based capture semantics.
- On a successful left press of a *different* interactive cap, queue the union of old and new clipped expanded border bounds before the next frame; retain existing raise, focus-endpoint reassertion, and local event ordering.
- On explicit destroy or owner cleanup, clear selected cap and capture if they identify the removed cap, queue its expanded bounds, and preserve the established owner/Grant cleanup protocol.
- A background surface must never set selected cap, receive decoration, capture, focus, or raise due to a desktop click.
- Do not add a public focus method, display metadata, client API, opcode, drag/resize region, or input hit region for the border.

## Architecture
`PointerRouter` owns `{ focused_owner, selected_cap, capture }`. Its successful activation reports prior and current caps through the existing narrow callback. `input_handler::route_pointer` resolves both private cap rectangles from `SurfaceTable`, uses Phase 01 geometry to accumulate outer-border damage, and preserves its `SetFocus` reassertion. The main message/owner-exit paths call a router cleanup method before removing a surface, then union raw freed rect plus border bounds. The renderer reads only `selected_cap`.

## Related Code Files
- Modify/split: `cells/services/compositor/src/pointer_router.rs` — add selected-cap accessor and removal cleanup; ≤200 lines after extracting target helpers if necessary.
- Modify/split: `cells/services/compositor/src/input_handler.rs` — adapt activation callback and expose selected cap to render wiring; ≤200 lines after extraction if necessary.
- Modify/split: `cells/services/compositor/src/main.rs` — pass input state to destruction/owner cleanup and renderer; extract IPC/lifecycle functions into bounded private modules before leaving it touched above 200 lines.
- Reuse: `cells/services/compositor/src/focus_decoration.rs`, `surface_table.rs`, and `z_order.rs`.
- Unchanged: `libs/{api,ostd}/src/display.rs`, `cells/tests/window-policy-probe/src/main.rs`, public input/display protocols.

## Implementation Steps
1. Extend `PointerRouter` with a private selected-cap field/accessor and a removal method. A removal clears `capture` only for that cap and clears selected cap without changing `focused_owner`, preserving current keyboard-owner behavior until the next valid click.
2. In the left-pressed, interactive-target branch, compare old/new selected caps. Keep the established selection → raise → dirty → SetFocus → local-send ordering; only report a decoration change when the selected cap changes.
3. In `input_handler`, map reported caps to live surface rects, expand/clip them through Phase 01 helpers, and union them into `pending_dirty`. Do not schedule decoration changes for move, scroll, release, miss, or background.
4. Thread `input.selected_cap()` into `render_frame` and update explicit destroy plus `cleanup_owner` to invoke router removal before the table entry disappears. Union the expanded bounds with the existing freed-area dirty.
5. Extract focused helpers/modules rather than allowing `main.rs`, `input_handler.rs`, or `pointer_router.rs` to exceed 200 lines; migrate all callers in the same change with no compatibility alias.

## Todo List
- [ ] Add private selected-cap state and accessor to pointer routing.
- [ ] Invalidate old/new outer bounds only for changed interactive selection.
- [ ] Clear selected/captured cap and invalidate outer bounds during destroy and owner-exit cleanup.
- [ ] Preserve role exclusion, focus reassertion, pointer capture, keyboard owner, and Grant lifetime ordering.
- [ ] Keep each new/touched implementation file at or below 200 lines.

## Success Criteria
- [ ] Clicking an exposed interactive surface raises it and causes exactly its selected-cap decoration to repaint.
- [ ] Reclicking the same selected cap does not create a distinct focus-decoration transition.
- [ ] Background clicks leave selected cap, z-order, capture, keyboard owner, and decoration unchanged.
- [ ] Destroying/owner-cleaning the selected cap leaves no border pixels; a removed captured cap still drops later captured events safely.
- [ ] No display API or client-visible surface coordinate behavior changes.

## Risk Assessment
If selection is tracked by owner rather than cap, two surfaces owned by one cell cannot be decorated correctly. If cleanup runs after table removal, expanded bounds cannot be recovered. Keep cap state internal, calculate dirty before removal, and do not reset `focused_owner` without explicit policy approval.

## Security Considerations
The selected cap is derived only from the existing interactive `hit_test` result. Ownership checks remain in compositor request handling; background roles remain restrictive. The change adds no capability transfer, client-controlled focus target, or Grant write path.

## Next Steps
Phase 03 extends the existing RV64 integration scenario with PPM samples proving visible border selection, old-border erasure, and intact client interiors.
