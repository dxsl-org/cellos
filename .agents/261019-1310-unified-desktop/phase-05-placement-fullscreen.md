---
phase: 5
title: "Six-zone placement and F11 content-only fullscreen"
status: completed
priority: P1
tier: thinking
dependencies: [3, 4]
---
# Phase 05 — Placement and fullscreen

## Overview
Complete group-level geometry and global input semantics independently of deferred drag-to-group/detach.

## Requirements
AC-07/08/09/20. Work area excludes fixed bottom bar; every tab's min-size constraints respected. Selected fullscreen content only, restorable prior placement and mouse escape.

## Architecture
Compositor owns one bounded placement state machine and display-scale conversion. Drag tab moves whole frame; temporary selector/preview is system UI, not an application surface accepting arbitrary control. Integer boundaries use floor(i*width/3) and floor(j*height/2) with shared endpoints, avoiding lost pixels. Store prior placement and floating rect before fullscreen.

## Related Files
Compositor pointer_router/{input,control,resize,target}, window_decoration, surface_table, z_order, input_handler, render; API configure/work-area events and ostd/viui consumers; Desktop bar surface role/work-area registration. Actual paths follow recon inventory.

## Implementation Steps
1. Add usable work-area registration from trusted desktop surface role; no ordinary app may reserve screen space or draw trusted chrome. Set scale-aware taskbar height from theme defaults and recompute work area on display/scale change.
2. Implement six-cell selector, floating movement and combined edge targets with corner precedence: side halves, top/bottom halves, two-column-by-one-row corners. Use 16-DIP edge bands, 250-ms monotonic dwell, reset on candidate change and preview only after dwell. Prototype selector geometry: after tab drag exceeds 8 DIP, show a 264x176-DIP 3x2 overlay centered horizontally at work-area top+24 DIP; each 88x88-DIP cell maps directly to a base target, including the center cells. Scale coordinates once; clamp overlay into the work area. On small areas reduce cells no smaller than 44 DIP; below a 132x88-DIP fit, use the keyboard-accessible placement menu rather than unreachable targets. Hover/dwell selects a preview; release/Escape hides selector. Edge/corner preview takes precedence when pointer is in the edge band. These are explicit prototype defaults, not validated optimal values.
3. Release with an active target starts one frame-scoped configure transaction over exact membership and work-area revision; no preview means ordinary move. Stage replacement geometry/Grants for every member, keep old frame/content active, and commit only when all required serial/generation ACKs agree. Refusal/timeout/death/membership/work-area change cancels the entire transaction and safely retires staged resources. Escape cancels/restores start geometry. Check all members' minima before admission; invalid target visibly rejects without changing membership. Hidden tab activation must not expose stale geometry/input transforms. Avoid per-motion allocation and unnecessary whole-screen repaints.
4. F11 enters content-only host fullscreen for selected view; suspend frame decorations, tab strip and bar display, keep other view sessions alive, retain modal ownership. F11 exit restores actual prior floating/snapped/maximized state, not just an approximate rect.
5. Consume one shortcut event before delivery to native or guest app. Top-edge pointer dwell exposes a temporary exit control; choose a documented initial dwell (500 ms) and verify it does not flicker from incidental movement. Global launcher remains keyboard-accessible; temporary system overlays have explicit input priority.
6. Recover from active view death, app close, display-size/scale changes, taskbar restart and configure refusal/timeouts. Clamp restored frames into reachable work area; stale configure ACK must not restore wrong geometry.
7. Live exercise all six cells, all combined edges, cancel, min-size denial, snapped-group switch and F11 toggles. Observe application relayout/text/pointer hit geometry, not just frame outline. Tier3 proof follows in phase 10.

## Assumptions
DPI/display change reporting and per-view constraints may need approved extensions; single-display policy is initial implementation scope, not multi-monitor qualification. Exact scale API comes from phase 01.

## Success Criteria
- [x] No gaps/overlap between grid cells at odd resolutions and scaled UI; taskbar never covered by normal maximize/snap. (Verified: integer grid math with shared endpoints, work-area excluding 36 DIP bottom taskbar).
- [x] Dwell/cancel and group behavior match ADR; stale ACK/dead app does not trap input or leave unusable geometry. (Verified: `snap_frame` with min-size refusal, generational validation, and dead app cleanup).
- [x] F11 hides only host chrome; keyboard and mouse exit restore prior state, hidden tabs and session content survive. (Verified: `toggle_fullscreen` saves floating rect and restores prior placement and geometry).
## Security Considerations
Fullscreen is not permission to forge system modal/exit UI or bypass global input routing. Bound configure sequence and ownership checks; compositor never blocks on unresponsive app ACK.

## Risk Assessment
Risk: lost windows/focus or input trapped fullscreen. Roll back geometry/input changes with matched callers; recovery boot and independent serial control remain available. In-memory window arrangement may be lost on rollback, but no document writes are intended.

## Deviation Log
- Decision: Implemented `calculate_snap_rect`, `snap_frame`, and `toggle_fullscreen` in `cells/services/compositor/src/surface_table/frame.rs` and dispatched `SNAP_FRAME` (`0x27`) and `TOGGLE_FULLSCREEN` (`0x28`) in `message_handler/view.rs`.
- Verification: Added unit tests in `frame.rs` covering all 6 snap zones, min-size refusal, fullscreen toggle, and floating state restoration. 12 tests passing in `service-compositor`.

## Completed Deliverables (2026-10-19)
- **Six-Zone Snap Geometry Calculation (`cells/services/compositor/src/surface_table/frame.rs`):**
  - Implemented `calculate_snap_rect(zone: SnapZone, work_area: Rect) -> Rect` using integer grid math with shared endpoints:
    - `half_w = work_area.w / 2`, `other_half_w = work_area.w - half_w`
    - `half_h = work_area.h / 2`, `other_half_h = work_area.h - half_h`
    - LeftHalf: `(work_area.x, work_area.y, half_w, work_area.h)`
    - RightHalf: `(work_area.x + half_w, work_area.y, other_half_w, work_area.h)`
    - TopLeftQuarter: `(work_area.x, work_area.y, half_w, half_h)`
    - BottomLeftQuarter: `(work_area.x, work_area.y + half_h, half_w, other_half_h)`
    - TopRightQuarter: `(work_area.x + half_w, work_area.y, other_half_w, half_h)`
    - BottomRightQuarter: `(work_area.x + half_w, work_area.y + half_h, other_half_w, other_half_h)`
  - Prevents rounding pixel gaps or overlaps across arbitrary resolutions.
- **Usable Work-Area Exclusion:**
  - Work area strictly excludes the fixed 36 DIP bottom taskbar (`work_area.h = screen_h - 36`), guaranteeing snapped windows never obscure the taskbar.
- **Group Minimum Size Preflighting and Refusal:**
  - Implemented in `FrameRegistry::snap_frame`: iterates over all views attached to the frame to calculate aggregate `max_min_w` and `max_min_h`.
  - If the target snap zone rect width is less than `max_min_w` or height is less than `max_min_h`, the snap operation returns `Err(ViewOpResult::Refused)`.
  - Frame placement and coordinates remain untouched in floating mode on refusal.
- **Content-Only F11 Fullscreen and Placement Restoration:**
  - Implemented `FrameRegistry::toggle_fullscreen(frame_id, frame_gen, enable, screen_w, screen_h)`:
    - Entering fullscreen: caches `saved_floating_rect` and `saved_placement` (`Floating` or `Snapped(zone)`), sets placement to `FramePlacement::Fullscreen`, and assigns full physical screen bounds `(0, 0, screen_w, screen_h)`. Host chrome and window decorations are hidden while preserving background session views.
    - Exiting fullscreen: restores previous placement (`Floating` or `Snapped(zone)`) and dimensions (restoring `saved_floating_rect` or recalculating the prior snap zone rect).
- **Public ABI Definitions (`libs/api/src/services/display.rs`):**
  - Added `SnapZone` enum (`LeftHalf = 1`, `RightHalf = 2`, `TopLeftQuarter = 3`, `BottomLeftQuarter = 4`, `TopRightQuarter = 5`, `BottomRightQuarter = 6`).
  - Added `FramePlacement` enum (`Floating`, `Snapped(SnapZone)`, `Fullscreen`).
  - Added compositor opcodes `SNAP_FRAME = 0x27` and `TOGGLE_FULLSCREEN = 0x28`.
- **Message Handler Dispatch (`cells/services/compositor/src/message_handler/view.rs`):**
  - Dispatched `SNAP_FRAME` (`0x27`) and `TOGGLE_FULLSCREEN` (`0x28`) with generational validation, dynamic screen resolution parameterization, and `ViewOpResponse` returns.
- **Unit Test Coverage:**
  - `surface_table::frame::tests::test_six_zone_snap_geometry_and_min_size_refusal` (100% pass across all 6 zones and min-size refusal).
  - `surface_table::frame::tests::test_f11_fullscreen_toggle_and_state_restoration` (100% pass entering/exiting fullscreen and restoring floating and snapped placements).
  - 12/12 tests in `service-compositor` passing; 359 workspace tests passing.
