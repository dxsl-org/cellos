# Phase 05 — Desktop Compositor Damage Clipping

## Context Links
`docs/display-api.md:138-157`; `docs/project-roadmap.md:68-85`; `cells/services/compositor/src/{render,framebuffer}.rs`; `tests/integration/tests/window-policy.rs`.

## Overview
Upgrade the existing bounded desktop compositor by making composition honor the calculated dirty region, avoiding full-surface copies for a small damaged area.

## Key Insights
`render_frame` correctly aggregates, clears, decorates, flushes, and invokes cursor composition for a dirty region, but `blit_surface` currently copies the entire intersecting surface. This wastes memory bandwidth and violates the damage-driven rendering intent without adding any shell functionality.

## Requirements
Preserve BGRA source-over order, screen clipping, decoration ordering, cursor correctness, and full existing window-policy behavior. No taskbar, snapping, persistence, or other desktop-shell features.

## Architecture
Render selects one dirty scanout rectangle. Each visible intersecting surface copies only its screen-space intersection with that region, then compositor-owned decorations and cursor paint/flush the same region.

## Related Code Files
`cells/services/compositor/src/render.rs`, `framebuffer.rs`, focused framebuffer/render tests, `tests/integration/tests/window-policy.rs`.

## Implementation Steps
1. Add a private clipped-blit primitive with correct source/destination coordinate translation.
2. Use it in damage-driven frame rendering.
3. Retain complete-surface rendering where needed through the same primitive.
4. Add deterministic pixel/edge clipping tests and preserve the QEMU window-policy route.

## Todo List
- [ ] Inspect framebuffer test seams.
- [ ] Implement clipped composition.
- [ ] Run focused compositor and window-policy verification.

## Success Criteria
A small dirty rectangle changes only the matching destination pixels from each intersecting surface; negative coordinates and screen edges remain correct; decoration/cursor ordering is unchanged.

## Risk Assessment
Incorrect source offsets create visual corruption at clipped or negative coordinates.

## Security Considerations
The compositor continues to read only validated app-owned surface buffers and retains owner/lifecycle boundaries.

## Next Steps
Run integration evidence; do not expand into a desktop shell.