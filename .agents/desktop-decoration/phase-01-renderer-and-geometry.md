---
phase: 1
title: "Focused-border renderer and clipped geometry"
status: complete
priority: P1
dependencies: []
tier: medium
---

# Phase 01: Focused-border renderer and clipped geometry

## Context Links
- `cells/services/compositor/src/render.rs:22-213`
- `cells/services/compositor/src/surface_table.rs:39-191`
- `cells/services/compositor/src/z_order.rs:1-46`
- `libs/api/src/services/display.rs:23-59`

## Overview
Create a compositor-private, one-pixel active-surface border and make the renderer correctly repaint its exterior pixels. The border is scanout-only: it surrounds rather than consumes client content.

## Key Insights
- `render_frame` already unions surface damage with `extra_dirty`, reblits bottom-to-top, composites the cursor last, and flushes one rect.
- `ScreenFb::blit_surface` only reads `SurfaceState::pixels()`; Grant-backed pixels are deliberately read-only at `surface_table.rs:117-131`.
- `Rect` offers union/intersection but no clipping helper, so clipping must remain private to the compositor rather than expanding the public display API.

## Requirements
- Define a fixed opaque BGRA focus color and one-pixel thickness in a compositor-private module; Phase 03 must use the matching RGB PPM value.
- Derive the outer border bounds with saturating/checked arithmetic, clip them to `[0,width) × [0,height)`, and skip empty results.
- Clear the clipped dirty rectangle before every reblend, then redraw all intersecting surfaces bottom-to-top. This must erase a removed/moved border in a scanout gap.
- Paint border pixels only in the screen framebuffer and only after blitting their selected cap; later z-order surfaces naturally occlude them and the cursor remains last.
- Do not mutate `SurfaceState`, `PixelSource`, a Grant buffer, surface dimensions, client coordinates, or `libs/{api,ostd}`.

## Architecture
Split renderer responsibility into small private modules (all ≤200 lines): a framebuffer module for allocation/blit/clear/flush, a render-loop module for dirty aggregation and stack traversal, and `focus_decoration.rs` for geometry plus border rasterization. The render-loop receives `Option<u64>` selected cap; during bottom-to-top traversal it blits each cap and, only for that cap, draws the clipped border inside the same dirty region. It composites the existing cursor after the full stack.

## Related Code Files
- Modify/split: `cells/services/compositor/src/render.rs` — preserve `render_frame` call semantics while reducing it to ≤200 lines.
- Create: `cells/services/compositor/src/focus_decoration.rs` — bounds, scanout clipping, and border rasterization; ≤120 lines.
- Create as needed: `cells/services/compositor/src/framebuffer.rs` — `ScreenFb` storage/blit/clear/flush; ≤190 lines.
- Modify: `cells/services/compositor/src/main.rs` only for any renderer module import/call adaptation; extract its existing message/lifecycle code before it remains touched above 200 lines.
- Unchanged: `libs/api/src/services/display.rs`, `libs/ostd/src/display.rs`, `surface_table.rs` pixel-source contract.

## Implementation Steps
1. Add `focus_decoration::{expanded_bounds, clip_to_scanout, paint_border}` with private `Rect` helpers. Use exclusive right/bottom edges and guard `i32` conversion/underflow before indexing framebuffer bytes.
2. Extract `ScreenFb` methods into a bounded framebuffer module if needed; add a clipped `clear_rect` that writes only compositor-owned screen pixels to the existing deterministic background value.
3. Amend `render_frame` to accept the private selected cap, aggregate normal damage plus selection invalidation supplied later, clip once before clear/reblend/flush, and return `None` for fully off-scanout dirt.
4. For every z-order cap intersecting dirty: blit client pixels, then conditionally paint that cap's decoration. Keep the existing damage clearing after reblend and cursor composition immediately before flush.
5. Split any touched implementation file that would exceed 200 lines; do not retain duplicate rendering paths or introduce a second framebuffer.

## Todo List
- [ ] Add private clipped focus-border geometry and rasterizer.
- [ ] Add dirty-rect clear/reblend support for exterior border removal.
- [ ] Thread selected-cap input through the bounded renderer and retain cursor ordering.
- [ ] Keep all new/touched renderer files within the 200-line limit.

## Success Criteria
- [ ] A one-pixel border occupies only the outer rectangle of a selected surface; the client rectangle is copied byte-for-byte as before.
- [ ] A dirty rect that includes a prior border restores its underlying stack/background after the border is absent.
- [ ] Partially and fully off-screen border bounds neither index outside `ScreenFb` nor cause a nonempty out-of-scanout flush.
- [ ] A higher z-order surface occludes a lower selected surface's border, and the cursor occludes every border pixel.

## Risk Assessment
Clearing a dirty region before reblit is required for exterior decoration removal but exposes an absent background as the compositor's defined background color. Reblit only the clipped dirty intersection to avoid extra work. Keep rectangle expansion local and checked: `Rect::union` itself does not provide clipping or overflow protection.

## Security Considerations
The helper writes only `ScreenFb::pixels`, never `SurfaceState::pixels()` or the read-only Grant pointer. Its selected cap comes from compositor state, not client IPC. No new capability, IPC opcode, metadata, or unsafe operation is required.

## Next Steps
Phase 02 supplies and clears the private selected cap and schedules old/new expanded border damage on activation and destruction.
