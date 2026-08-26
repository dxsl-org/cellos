---
title: "Compositor-rendered focused-surface decoration"
status: complete
created: 2026-08-25
scope: compositor-only visual focus border
---

# Compositor-rendered focused-surface decoration

## Context Links
- `7d5baabc` (`feat(desktop): add bounded window focus policy`)
- `826f32f9` (`fix(desktop): route pointer input through compositor`)
- `cells/services/compositor/src/{render.rs,input_handler.rs,pointer_router.rs,main.rs,surface_table.rs,z_order.rs}`
- `tests/integration/tests/window-policy.rs`

## Overview
Add a one-pixel compositor-owned border around the selected interactive surface. It is painted into the scanout framebuffer, never into a client Grant, and changes only after a successful interactive left-click selection.

## Key Insights
- Damage, back-to-front compositing, z-order, hit-testing, capture, and keyboard-owner routing already exist.
- `SurfaceRole::Background` is visible but excluded by `pointer_router::hit_test`; decoration must retain that exclusion.
- A border outside a surface needs expanded damage and a dirty-region clear/reblend to erase the old border even where no client surface covers it.

## Requirements
- Preserve client content coordinates, `ViSurface` API, Grant zero-copy/read-only ownership, capture, and keyboard-focus behavior.
- Clip all decoration and repaint work to scanout; draw the focus layer after its surface in paint order and before the cursor.
- Clear focus decoration on selected-surface removal; never make a border hit-testable, draggable, resizable, titled, or closable.
- Keep every new or touched implementation file at or below 200 lines; split focused helpers instead of extending legacy oversized modules.

## Architecture
`left press → PointerRouter selected cap → expanded old/new dirty → renderer clear/reblend → focused cap's border → cursor → flush`. The selected cap remains private compositor state; client surfaces and display IPC remain unchanged.

## Related Code Files
- Modify/split: `cells/services/compositor/src/{main.rs,render.rs,input_handler.rs,pointer_router.rs}`.
- Create: focused compositor-private rendering/lifecycle helper modules, each ≤200 lines.
- Modify: `tests/integration/tests/window-policy.rs` (remain ≤200 lines); reuse `cells/tests/window-policy-probe` unchanged because it already exercises selection.
- Modify: desktop roadmap, architecture, changelog, and personal TODO status. CI needs no workflow change: its named `window-policy` test now carries the added assertions.

## Implementation Steps
1. Establish private border geometry, clipped dirty clearing, and correct surface/border/cursor paint order.
2. Track selected cap internally; invalidate both border extents on selection and removal without changing input routing contracts.
3. Extend the existing RV64 QEMU probe assertion to observe initial absence, selected-border switch, restored old pixels, and unchanged client interiors.

## Phase Status / Progress
| Phase | Status | Progress | Dependency |
|---|---|---:|---|
| [01 renderer and geometry](phase-01-renderer-and-geometry.md) | complete | 100% | — |
| [02 selection and lifecycle](phase-02-selection-and-lifecycle.md) | complete | 100% | 01 |
| [03 RV64 QEMU evidence](phase-03-rv64-qemu-evidence.md) | complete | 100% | 01, 02 |

## Success Criteria
A click-selected interactive surface has a clipped, one-pixel visible border; selecting another removes the old border and paints the new one without changing any client pixel. The cursor remains topmost, backgrounds remain noninteractive, and the QEMU PPM assertion is deterministic.

## Risk Assessment
Border removal can leave stale scanout pixels; border-over-top rendering can violate z-order; edge/off-screen surfaces can overflow or flush invalid rectangles. The phases explicitly use clipped geometry, clear/reblend, and in-stack decoration painting.

## Security Considerations
Selection derives only from existing compositor hit targets and owner-checked surfaces. No client-supplied focus target, new capability, Grant write, metadata, or privileged operation is introduced.

## Next Steps
Completed with `cargo check -p service-compositor`, disk packaging/signing, and
the RV64 `window-policy` and `compositor-cursor` QEMU tests. Defer title
metadata, titlebars, dragging, resize, close controls, desktop shell, taskbar,
and all public display API expansion.
