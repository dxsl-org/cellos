---
phase: 6
title: "Ocel native session-tab and shared reader cutover"
status: pending
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
- [ ] Actual Ocel pages live in one native system-tab layer and retain independent navigation/loading state after reorder/group/detach.
- [ ] Follow-system and explicit reader choices pass visible all-palette scenarios without app restart; original images/PDF unchanged.
- [ ] F11 round trip preserves page/scroll/input state; close during load cannot mutate another view or leak service lease.

## Security Considerations
Preserve existing resource budgets, isolated JS/PDF and certificate policy. System-tab grouping is not script cross-document access. No data execution from filenames or forged launch metadata.

## Risk Assessment
Risk: active-view assumptions and document background work cause cross-view state leaks. Roll back Ocel and matched SDK/protocol/image together; user document data must not be migrated by this phase. In-memory browsing sessions may be lost on rollback; use disposable fixtures for fault cases.

## Deviation Log
Append during execution.
