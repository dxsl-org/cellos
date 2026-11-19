# Cell Tab value — research (2026-11-11)

## Question
The Haiku-style Cell Tab (icon + title, narrower than the frame) only pays off
through features built on it. Which ones, in what order, on today's code?

## Evidence
- Haiku user guide, "Haiku's GUI" (https://www.haiku-os.org/docs/userguide/en/GUI.html):
  the tab is used for (1) Shift+drag to slide it along the top edge so stacked
  windows stay reachable by their tab, (2) double-click to minimize, right-click
  to send back, (3) Stack & Tile: Opt+drag a tab onto another tab = stack
  (tabs auto-arranged side by side, one frame), Opt+drag near a border = tile
  (shared borders); groups move/resize/hide/raise together.
- Zeidler, Lutteroth, Weber 2013, "An Evaluation of Stacking and Tiling
  Features within the Traditional Desktop Metaphor"
  (https://www.cs.auckland.ac.nz/~lutteroth/publications/ZeidlerEtAl2013-StackAndTile.pdf):
  30-person controlled study — multi-window tasks and task switching
  significantly faster with Stack & Tile (all tasks p < 0.01 except the first
  look-up across 3 apps); setup cost recouped; 146-user survey: stacking used a
  bit more than tiling (55/45 %), groups mostly 2+ windows stacked, main uses
  programming, browsing, file management; asks for taskbar grouping by group and
  group persistence.
- ADR-0027 already specifies: tab = icon + app name; icon menu with session
  actions, grouping/detaching, placement; `+` for a new session; explicit menu
  commands for grouping first (drag-to-group deferred).

## Current code (FrameMap scout, file:line in its report)
- `FrameRegistry` (compositor `surface_table/frame.rs`) models frames with N
  views, selected view, placement, generation; ops CREATE/DESTROY/SELECT/GROUP/
  DETACH_VIEW, SNAP_FRAME, TOGGLE_FULLSCREEN exist but no client sends them,
  `CREATE_SURFACE` windows get no frame, and render / hit-test / z-order / the
  window menu never consult frames. Ops are owner-gated, so mixed-app frames
  (the point of Stack & Tile) are impossible today. STAGE/COMMIT_GROUP_CONFIGURE
  have opcodes only.

## Value-ordered features
1. **Stacking (core).** Every window gets a one-tab frame; frames draw N tabs
   side by side on one body (each tab = icon + title of its view); click a tab
   to select it; non-selected views hidden; move/resize/minimize/maximize apply
   to the frame; window menu gains "Stack onto <window below>" and "Detach".
   Compositor-only, no ABI (user gestures bypass the owner gate inside the
   compositor; IPC ops stay gated).
2. **Drag to stack / detach.** Drag a tab onto another frame's tab strip
   (highlight target) to stack; drag a tab out to detach. Same model as 1.
3. **Sliding tab.** Shift+drag a tab along its frame's top edge so overlapping
   single-tab windows keep a visible, clickable tab.
4. **Tiling.** Glue frames by shared borders; group moves/resizes together.
   Larger (constraint solving); after 1–3.
5. **Taskbar / Spotlight "Active" by group**, group persistence — follow-ups
   the survey asked for.

## Risks
- Hidden views must not receive input or stale geometry (contracts.md:31):
  switching tabs configures the incoming view to the frame size before showing.
- Close in a group closes one tab; closing the group needs every member's
  consent (ADR-0027) — keep Close per tab in phase 1.
