---
phase: 5
title: "Six-zone placement and F11 content-only fullscreen"
status: pending
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
- [ ] No gaps/overlap between grid cells at odd resolutions and scaled UI; taskbar never covered by normal maximize/snap.
- [ ] Dwell/cancel and group behavior match ADR; stale ACK/dead app does not trap input or leave unusable geometry.
- [ ] F11 hides only host chrome; keyboard and mouse exit restore prior state, hidden tabs and session content survive.

## Security Considerations
Fullscreen is not permission to forge system modal/exit UI or bypass global input routing. Bound configure sequence and ownership checks; compositor never blocks on unresponsive app ACK.

## Risk Assessment
Risk: lost windows/focus or input trapped fullscreen. Roll back geometry/input changes with matched callers; recovery boot and independent serial control remain available. In-memory window arrangement may be lost on rollback, but no document writes are intended.

## Deviation Log
Append during execution.
