---
phase: 3
title: "Decoration rendering and pointer policy"
status: pending
priority: P1
effort: 1d
dependencies: [1, 2]
tier: thinking
---

# Phase 03: Decoration rendering and pointer policy

## Overview
Replace the completed focus-only exterior border with a compositor-owned, clipped titlebar/frame layer and route only decoration hit zones to window-manager actions. Client content coordinates and input behavior remain unchanged everywhere else.

## Requirements
- Functional: draw bounded title/frame/control pixels, title text, active/inactive state, and minimize/maximize/close controls; support titlebar drag and all edge/corner resize directions with compositor capture.
- Non-functional: all decoration writes target only `ScreenFb`; every old/new decorated extent is damaged; stack ordering is surface-content then that surface's decoration, with cursor last.
- No regression: `SurfaceRole::Background` is neither decorated nor hit-testable; input in content retains the existing select/raise/focus/capture/local-coordinate forwarding contract.

## Architecture
Make `window_decoration.rs` the single private geometry/raster module and remove the obsolete focus-only renderer once callers migrate. For content `C=(x,y,w,h)`, use constants `FRAME=4`, `TITLE=20`, `CONTROL=16`, and a decorated frame `F=(x-4,y-24,w+8,h+28)` with saturating endpoints. Clip every returned rect to the scanout before index/flush.

The titlebar is `F`'s top 20 pixels. Place 16×16 close, maximize/restore, and minimize cells right-to-left inside it with a 2-pixel inset; the remaining titlebar interior is the drag zone. The outer 4-pixel bands are resize zones: corners first, then top/bottom/left/right; the top band remains a resize edge and the titlebar below it remains draggable. Hit-test priority is **controls → resize corner → resize edge → title drag → client content**. A point outside `F`, or on a background/minimized/closing cap, has no window-manager target.

`PointerRouter` adds a private capture mode: `Client(cap)`, `Drag{cap,grab_x,grab_y}`, or `Resize{cap,edge,start_pointer,start_rect}`. Decoration press selects/raises/reasserts compositor focus before entering its mode and is consumed (no forwarded client press); drag moves `C.x/y` by preserved grab offset and damages old/new `F`; resize computes a min-size-checked requested content rect and delegates to Phase 2's single pending-configure/coalescing machinery. Control release triggers only if its press/release remain on the same control/cap. Client-content capture and all forwarded local coordinates remain as today.

Render traversal becomes `for cap bottom→top: if visible and intersects dirty { blit content; paint that cap's decoration }`; inactive and selected colors are compositor constants, title glyphs use a bounded fallback renderer, and later surfaces occlude earlier frames naturally. Clear/reblend `dirty`, clear normal damage after paint, then composite the current cursor and flush once. Every title, move, selection, state, z-order, configure commit, and removal invalidates the union of old/new `F`; do not promote decorations to separate z-order entries.

## Assumptions
- **Claim:** a small fixed bitmap/glyph helper is available or can be extracted without depending on client-side ViUI rendering.
  **Confidence:** medium
  **How to verify:** inspect existing compositor/font dependencies; otherwise implement the project-standard fixed fallback glyph table in the private module.

## Related Files
- Replace/remove: `cells/services/compositor/src/focus_decoration.rs` — migrate all callers to `window_decoration.rs`; retain no duplicate frame path.
- Create: `cells/services/compositor/src/window_decoration.rs` and, if needed, a small pointer-mode helper; each ≤200 lines.
- Modify/split: `cells/services/compositor/src/render.rs`, `pointer_router.rs`, `input_handler.rs`, `surface_table.rs`, and `main.rs`.
- Modify only through Phase 1 API: `libs/api/src/services/display.rs` for shared geometry-independent state names if needed.

## Implementation Steps
1. Implement checked frame/title/control/edge geometry, clipping, and scanout-only rasterization. Reject empty/overflowed content rects and render unsupported title glyphs as a deterministic replacement glyph.
2. Change render ordering from global blits plus selected border to per-cap content-plus-decoration while retaining dirty aggregation, background clear, hardware/software cursor behavior, and one flush.
3. Add decoration target lookup and pointer capture modes. Keep the current content `hit_test` and forwarding as the final branch; forbid background, hidden, closing, and pending-invalid caps at the entrance.
4. Wire controls to the Phase 2 state machine, including the maximize/restore icon selected from presentation state. On resize motion coalesce rather than emit an unbounded configure stream; cancel decoration capture on timeout/destroy/minimize/close.
5. Schedule exact old/new frame damage for all transitions, z raises, and removal; confirm a top surface still occludes lower frame/title pixels and cursor remains topmost.

## Task List
- [ ] Replace the focus-only path with bounded frame/title/control rendering.
- [ ] Interleave decoration painting with each surface's z-order layer.
- [ ] Add decoration hit zones and mode-tagged compositor capture.
- [ ] Coalesce resize requests and invalidate every old/new frame extent.

## Success Criteria
- [ ] Interactive surfaces have compositor-rendered, clipped titlebars and controls; their Grant pixels and local `(0,0)` content origin are unchanged.
- [ ] Drag moves only from the drag zone; every edge/corner proposes the correct anchored content geometry; control clicks do not leak pointer presses/releases to the client.
- [ ] Resize leaves the old frame/content visible until Phase 2 commits the matching replacement Grant, and one outstanding configure bounds all resize traffic.
- [ ] Minimized/closing/background surfaces have no decoration interaction; a higher z surface and cursor occlude lower decorations correctly.
- [ ] Verification: `cargo test -p service-compositor && cargo test -p api -p ostd`.

## Security Considerations
Decoration targets are derived only from compositor-owned z-order/state and kernel-authenticated owners. Client title bytes are rendered after bounded validation, never interpreted as format strings or pointers. Geometry uses checked/saturating calculations before framebuffer addressing; no pointer route permits an owner to move, resize, close, or alter another owner's surface.

## Risk Notes
The current renderer paints all client surfaces before the selected border, so minimally adding a titlebar there would violate occlusion. Decoration capture must not reuse the client capture field without a mode tag, or a titlebar drag can incorrectly forward release/motion. The no-preview rule is deliberate: rendering a proposed larger client area would fabricate pixels the client has not supplied.

## Deviation Log
None.
