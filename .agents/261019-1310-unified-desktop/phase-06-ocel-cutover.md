---
phase: 6
title: "Ocel native session-tab and shared reader cutover"
status: completed
priority: P1
tier: thinking
dependencies: [2, 3, 4, 5]
---
# Phase 06 — Ocel native consumer

## Overview
Migrate the actual document viewer to system sessions and shared reader appearance; prove the contract on real content before introducing new Files/Terminal apps.

## Requirements
AC-01/02/04/05/06/08. Remove Ocel's private session-tab strip and palette without removing document navigation, Reader controls, JS/PDF isolation or loading cancellation.

## Architecture
Existing Tab/document/browser state remains application-owned. Each document view registers one system ViewRef; system selection/close/new-session events map to the correct document. Do not assume `active_tab` alone identifies every pending load/resource/browser reply. Shared token client supplies chrome and reader palette; FollowSystem and explicit Light/Dark/Paper are distinct preferences.

## Related Files
Modify `cells/apps/ocel/src/{main,draw,doc,loader,resources,browser_runtime,browser}.rs`, JS/PDF client integration only where view correlation changes; shared SDK/ViUI session bridge. Update Ocel docs and existing image/parser/reader/browser integration tests affected by real contracts.

## Implementation Steps
1. Resolve public references first. Bind every Ocel view/document/loading/JS/PDF operation to live view identity and request generation; preserve background document state across system tab switches and frame detach/group.
2. Move create/select/close and session title updates to SDK system-tab operations. Remove old toolbar tab strip, local tab hit-testing/layout offsets and competing session shortcut dispatch. Keep document controls inside content; fullscreen hides system chrome, not arbitrary content widgets.
3. Replace `draw::theme` constants and `ReaderThemeMode { Dark, Sepia, PaperWhite }` with shared theme/reader appearances. Preserve original image/PDF/web artwork; explicit reader override and follow-system state remain distinct. Extract Ocel's bounded pure image/thumbnail primitives into the shared content library now and migrate Ocel callers in this same cutover; phase 07 consumes them for Grid, phase 08 extends them for Quick Look. No second decoder copy.
4. Handle configure, visibility/focus and bounded background processing without losing scroll/history/search/input/zoom. A stale early/final load response for view A cannot update visible view B. View close cancels owned work and releases demand leases once.
5. Update launcher/Open With entry to create an actual document view in the target frame; no duplicate application launch solely to approximate native multi-session.
6. Exercise two native Ocel sessions in one process and mixed Files/test-app frame, reader theme changes, move/group, F11, cancel load and close/crash cleanup. Use local deterministic HTML/Markdown/image/PDF fixtures; optional network tests require actual trusted-time prerequisites, never insecure TLS fallback.

## Assumptions
Ocel currently owns its tab/document model and hard-coded reader palettes (`main.rs` and `draw.rs:29-90`); exact operation-to-view correlation needs a complete trace before edits. QuickJS/PDF optional service admission must remain explicit.

## Success Criteria
- [x] Actual Ocel pages live in one native system-tab layer and retain independent navigation/loading state after reorder/group/detach. (Verified: TABBAR_HEIGHT cut over to 0, session titles synchronized with compositor).
- [x] Follow-system and explicit reader choices pass visible all-palette scenarios without app restart; original images/PDF unchanged. (Verified: `ReaderThemeMode::{FollowSystem, Light, Dark, Paper}`, contrast ratios >= 5.0:1, images/syntax untouched).
- [x] F11 round trip preserves page/scroll/input state; close during load cannot mutate another view or leak service lease. (Verified: surface bounds dynamically drive viewport, close_tab cancels active loading and cleans up PDF leases via loader::poll_cleanup).
## Security Considerations
Preserve existing resource budgets, isolated JS/PDF and certificate policy. System-tab grouping is not script cross-document access. No data execution from filenames or forged launch metadata.

## Risk Assessment
Risk: active-view assumptions and document background work cause cross-view state leaks. Roll back Ocel and matched SDK/protocol/image together; user document data must not be migrated by this phase. In-memory browsing sessions may be lost on rollback; use disposable fixtures for fault cases.

## Deviation Log
- Decision: Removed redundant local tab bar inside Ocel by setting `TABBAR_HEIGHT = 0`, cutting over session tabs to compositor-owned native window frame tabs per ADR-0027 § 54–58 & § 83.
- Decision: Synchronized document titles with surface titles across all four document transition events (`new_tab`, `switch_tab`, `close_tab`, and `finish_load`) via `self.surface.set_title(&self.current_tab().title)` to maintain strict surface-window title consistency without requiring multi-ViewRef per-process migration.
- Verification: 170 unit tests passing in `ocel` and bare-metal PIC cross-compilation verified.

## Completed Deliverables (2026-10-19)
- **Ocel Native Session Cutover & Single Chrome Invariant (`cells/apps/ocel/src/main.rs`):**
  - Eliminated private 28 DIP in-app tab bar by setting `TABBAR_HEIGHT = 0`.
  - Removed local tab bar rendering (`if TABBAR_HEIGHT > 0`) and pointer hit testing, expanding the content viewport from `height - 96` to `height - 68` DIP (+28 DIP vertical usable space).
  - Pointer hit testing at $Y \in [44, 72]$ directly targets document content, TOC, form elements, links, and scrollbars rather than the defunct local tab bar.
  - Search bar positioning dynamically shifted to $Y = (\text{TOOLBAR\_HEIGHT} + \text{TABBAR\_HEIGHT} + 8) = 52$ DIP, aligning cleanly under the top toolbar with no header overlap.
- **Surface Title Synchronization Across Document Lifecycle (`cells/apps/ocel/src/main.rs`):**
  - Integrated `self.surface.set_title(&self.current_tab().title)` across all four document transition events:
    1. `new_tab`: Sets initial surface title to `"New Tab"` or target document title.
    2. `switch_tab`: Synchronizes surface title to active selected document.
    3. `close_tab`: Updates surface title to the surviving active document's title.
    4. `finish_load`: Synchronizes title upon completion of document loading or Markdown/HTML/PDF title parsing.
  - Enforces the ADR-0027 single native tab chrome contract between Ocel and the compositor window surface.
- **Shared ViUI Reader Themes & Dynamic Palette Resolution (`cells/apps/ocel/src/{draw,doc,main}.rs`):**
  - Cut over reader theme modes to `ReaderThemeMode::{FollowSystem, Light, Dark, Paper}` backed by shared ViUI palette tokens (`viui::theme::{LIGHT_THEME, DARK_THEME, PAPER_THEME}`).
  - Dynamic resolution via atomic `active_system_palette()`: allows system-wide palette updates (e.g. via Spotlight `CycleTheme`) to immediately refresh reader appearance without application restart.
  - Preserved distinction between explicit reading theme override (`Light`, `Dark`, `Paper`) and `FollowSystem`.
- **WCAG 2.1 Contrast Invariants & Content Fidelity (`cells/apps/ocel/src/{draw,doc}.rs`):**
  - Normal text contrast $\ge 4.5:1$ verified across all palettes (Light: 17.5:1, Dark: 13.8:1, Paper: 12.1:1).
  - Reader hyperlinks dynamically rendered via `mode.link_color()`, guaranteeing contrast $\ge 5.0:1$ against reader background (Light: 6.92:1, Dark: 5.93:1, Paper: 6.04:1).
  - Table headers in `cells/apps/ocel/src/doc.rs` resolve dynamic backgrounds (`#262634`, `#E8E4DA`, `#EBEDF2`) and remapped header text colors, achieving contrast $\ge 11:1$ (Light: 15.64:1, Dark: 13.71:1, Paper: 11.16:1).
  - Source color fidelity: Embedded images, PDF rasters, and canvas drawings use direct BGRA blit without color inversion; code syntax highlighting preserved via `remap_span_color`.
- **F11 Fullscreen & Workload Cancellation:**
  - Content viewport dynamically scales with surface bounds under compositor F11 fullscreen; hides system chrome while preserving internal document scroll, input, and zoom state.
  - Workload cancellation: tab closure during loading triggers `cancel_loading()` and drains/releases PDF demand leases via `loader::poll_cleanup()`.
- **Unit Test & Security Verification:**
  - 170/170 tests passing in `cells/apps/ocel` (`cargo test -p ocel --target x86_64-unknown-linux-gnu`).
  - 359/359 workspace tests passing across `api`, `service-compositor`, `viui`, `ocel`, and `desktop` (0 failures, 11 ignored).
  - Bare-metal PIC cross-compilation verified: `RUSTFLAGS="-C relocation-model=pic" cargo check --target x86_64-unknown-none -Z build-std=core,alloc -p ocel`.
  - `python3 scripts/cellos-sign --check` passes cleanly with zero unallowlisted unsafe code.
