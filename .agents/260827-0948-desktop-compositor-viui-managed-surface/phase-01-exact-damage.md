# Phase 01 — Exact damage

## Context Links
Approved spec AC-2/AC-3; `libs/viui/src/renderer.rs`; `libs/viui/src/dirty.rs`.
## Overview
Translate ViUI float damage to clipped surface-local integer damage.
## Key Insights
`None` means full surface; empty or wholly offscreen rectangles emit no notification.
## Requirements
Preserve drawing and headless behavior; avoid API/protocol changes.
## Architecture
Keep conversion inside `FramebufferRenderer`, immediately before `ViSurface::damage`.
## Related Code Files
`libs/viui/src/renderer.rs`.
## Implementation Steps
Add conversion helper, submit exact/clipped damage, add boundary tests.
## Todo List
- [x] Implement clipping/rounding.
- [x] Add tests.
## Success Criteria
Signal damage is non-full and exact; full/empty/offscreen cases match AC-2/AC-3.
## Risk Assessment
Fractional bounds must expand outward without overflow.
## Security Considerations
Saturating bounded conversion prevents malformed IPC geometry.
## Next Steps
Expose managed lifecycle without changing `libs/api`.
