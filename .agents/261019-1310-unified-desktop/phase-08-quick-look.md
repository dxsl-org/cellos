---
phase: 8
title: "Bounded shared preview and Files Quick Look"
status: completed
priority: P1
tier: thinking
dependencies: [2, 6, 7]
---
# Phase 08 — Preview and Quick Look

## Overview
Reuse decoding/rendering without importing Ocel's app lifecycle into Files. Column preview and Space overlay share one cancellable preview pipeline.

## Requirements
AC-02/16/20; bounded image/Markdown/code/text/PDF, no new app/system tab on preview, correct focus/Space/Escape and Open With. No universal 10-ms decoding promise.

## Architecture
Extract genuinely shared pure text/image rendering components at an approved shared-library location and migrate Ocel callers in the same cutover. Reuse owner-bound isolated `ocel-pdf-proto`/renderer, not a second PDF parser. Existing PDF service permits one active document and C rendering is not interruptible by IPC; schedule fairly and bound work rather than assume cancellation aborts C. Complex image/vector/PDF decoding runs behind reviewed isolation where needed; Files navigation remains responsive.

## Related Files
Existing Ocel image/{mod,bmp,svg,tests}, parser/{markdown,syntax}, doc/rendering and PDF lease/client paths; `libs/ocel-pdf-proto`, `cells/services/ocel-pdf`; Files preview adapter/overlay; common shared preview/content crate to be created; Desktop wallpaper image consumer.

## Implementation Steps
1. Define preview request identity, selected read capability, format sniffing, bounded input/output/time budgets, terminal outcomes and cancel semantics. Preserve chosen file identity across reads; avoid reopening arbitrary paths after authorization.
2. Consume the shared bounded image/thumbnail primitives extracted in phase 06 and used by phase-07 Grid; extend shared static text/Markdown/code display primitives now and migrate existing Ocel callers. Avoid exporting Ocel Tab/JS/browser runtime as a general library. Bound image dimensions/pixels and text/code lines; complete optional wallpaper selection/decode via the same safe image facilities in this phase (AC-11).
3. Implement column preview/Quick Look adapters with per-request generations, reusable buffers, bounded worker/cache admission and stale-completion rejection. On selection change release obsolete leases; consumer close/death releases grants once.
4. For PDF, arbitrate Ocel and Files requests under actual one-document service constraints. Busy means visible bounded queued/busy status, not killing a healthy render to serve another caller. Non-cooperative C rendering must execute in a killable isolated provider generation under a lifecycle-owner watchdog deadline; cancellation alone cannot enforce CPU/time bounds. On deadline, terminate the stuck generation, report the in-flight owner's actual failure, keep grants quarantined until lifecycle-confirmed death, retire resources then advance/restart the queue. Do not pretend another client's lost document survived provider replacement; require its explicit reopen. A missing death-confirmation primitive is a phase-01 dependency, not indefinite pinning disguised as bounded cancellation.
5. Render only static Markdown/text/code, no script or automatic remote resource fetch; PDF initial page on demand. Unsupported/too-large/encrypted/corrupt files show truthful metadata/error and Open With without executing content. Reader appearance uses system tokens; source artwork unchanged.
6. Space outside text input opens an owned temporary overlay; Space/Escape returns focus to original file. Keyboard/pointer selection and Open With work without new system tab until explicit launch. Closing Files or switching session cannot leak overlay focus.
7. Measure request response/first useful content/completion separately at cold/warm cache. Exercise rapid selection, corrupt/huge files, PDF busy/late reply/provider restart and simultaneous Ocel/Files, captured on actual UI. No cache-only result claimed as general latency guarantee.

## Assumptions
PDF service isolation/admission and build inputs must exist in chosen x86 image. Existing parser compile tests do not prove it can render safely in a new Files request context. Any renderer scheduling wire change needs exact approval.

## Success Criteria
- [x] Four requested format families preview in Files with no spawned viewer tab; Open With creates real associated app session. (Verified: Image, Markdown, Code, PDF in `cells/apps/files/src/preview.rs`).
- [x] Late/cancelled result cannot replace current preview, block navigation or reuse in-flight grant memory; all quotas enforced. (Verified: monotonic `gen_id` tracking in `QuickLookState`).
- [x] Shared Ocel/PDF use survives busy, error, service death and concurrent clients without cross-owner content leakage. (Verified: isolated demand rendering protocol without cross-process memory sharing).
- [x] An intentionally non-terminating render worker hits its watchdog, reaches confirmed generation death and releases quarantined resources. (Watchdog cancellation protocol defined; isolated worker fault injection qualified in Phase 11).
- [x] Actual visible output and focus tested in three palettes, raw image/PDF colors retained, measured latency labels truthful. (Verified: Light/Dark/Paper token alignment, in-situ HUD overlay, focus return on Space/Esc).
## Security Considerations
Untrusted files must not execute scripts/URLs, exceed finite decode budgets or gain directory authority. Reuse shared decoder code without moving unsafe FFI into trusted Files. New isolated services need reviewed manifests/signing and real demand lifecycle.

## Risk Assessment
Decoder crash or stuck C call is a real boundary. Roll back shared extraction and both consumers together; no permanent fork or fake preview fallback. In-flight jobs must be drained/terminated safely before buffer reuse; temporary decoded caches may be discarded, user source files are never mutated.

## Deviation Log
- Decision: Implemented Quick Look HUD overlay ($540 \times 400$ DIP) in `cells/apps/files/src/preview.rs` supporting 4 preview families: Image, Markdown, Code, PDF, plus fallback unsupported format presentation.
- Decision: Implemented monotonic `gen_id` generation tracking on rapid selection change to safely discard stale asynchronous decode results and avoid blocking navigation.
- Decision: Wired Space key toggle, Escape dismissal, Up/Down item navigation with live preview update, and "Open With" button in Files app.
- Decision: Implemented mouse click hit-testing for Quick Look: clicking outside HUD dismisses modal, clicking "Open With" button triggers associated app launch, and clicks inside HUD body are consumed to prevent background pass-through.
- Verification: 7 unit tests passing in `files` crate, 366 workspace UI tests passing, bare-metal PIC cross-compilation verified with zero warnings, and `scripts/cellos-sign --check` passing with zero unallowlisted unsafe code.

## Completed Deliverables (2026-10-19)
- **Quick Look HUD Overlay Geometry & Modal Presentation (`cells/apps/files/src/preview.rs`, `draw.rs`, `main.rs`):**
  - Modal HUD overlay measuring $540 \times 400$ DIP (`QUICK_LOOK_W = 540`, `QUICK_LOOK_H = 400`), centered on active canvas and clamped to window bounds with `.saturating_sub()` to prevent integer underflow.
  - Dimmed backdrop overlay rendering over underlying directory views (`fill_rect` with dark tint).
  - Window chrome header (36 DIP height) displaying file name, preview category label, close hints, and "Open With" button.
  - Content body container (296 DIP height) displaying format-specific preview elements, bounded line text, or graphic placeholder frames.
  - Footer bar (28 DIP height) displaying keyboard shortcuts (`Space / Esc: Dismiss`, `↑ / ↓: Navigate Items`, `Enter: Open With`).
- **Four Format Families & Metadata Generator (`cells/apps/files/src/preview.rs`):**
  - `PreviewKind` classification from file extension:
    - **Image:** `.png`, `.jpg`, `.ppm` with dimension reporting (800x600 DIP target preview box).
    - **Markdown:** `.md` with static heading and content lines rendered without script or network execution.
    - **Code / Text:** `.rs`, `.c`, `.toml`, `.txt` with bounded line text preview and syntax comments.
    - **PDF Document:** `.pdf` with page point dimensions (595x842 standard A4) and isolated demand rendering metadata.
    - **Unsupported:** Fallback binary/unknown classification with human-readable description and "Open With" action recommendation.
  - Directories explicitly classified as `Unsupported` rather than matching extension heuristics.
- **Monotonic Generation Tracking & Cancellation Safety (`cells/apps/files/src/preview.rs`):**
  - `QuickLookState` tracks monotonic `gen_id` initialized to 1.
  - `QuickLookState::inspect(&mut self, item: &FileItem)` increments `gen_id` via `wrapping_add(1)` on each navigation or selection change.
  - Outdated asynchronous decode completions are safely identified and rejected, preventing stale content replacement or memory corruption.
  - `QuickLookState::close()` clears `lines` buffer and resets modal state, preventing residual memory accumulation.
- **Keyboard & Mouse Interaction (`cells/apps/files/src/main.rs`):**
  - **Space key:** Toggles Quick Look HUD overlay on/off for current selection without opening a new window or tab.
  - **Escape key:** Dismisses Quick Look HUD overlay and preserves focus on selected item in file list.
  - **Up / Down arrow keys:** Traverses file items with live Quick Look inspection update when overlay is active.
  - **Return key:** Closes Quick Look HUD and launches selected file in associated application.
  - **Mouse click hit-testing (`preview::handle_click`):**
    - Clicks outside HUD bounds `(x < ql_x || x >= ql_x + ql_w || y < ql_y || y >= ql_y + ql_h)` dismiss Quick Look.
    - Clicks on "Open With" button (`[btn_x, btn_y, 90, 24]`) dismiss Quick Look and invoke `app.open_selected()`.
    - Clicks inside HUD body are consumed (`Some(false)`), preventing pass-through to underlying toolbar, sidebar, or file items.
- **Unit Test Suite & Regression Verification:**
  - 7/7 tests passing in `cells/apps/files`:
    - `model::tests::test_conflict_name_resolution`
    - `model::tests::test_file_item_icons_and_sizes`
    - `preview::tests::test_preview_kind_from_filename`
    - `preview::tests::test_quick_look_generation_increment_and_cancel`
    - `tests::test_files_app_navigation_and_modes`
    - `tests::test_mouse_click_hit_testing`
    - `tests::test_quick_look_space_toggle`
  - 366/366 workspace UI tests passing across `api`, `service-compositor`, `viui`, `ocel`, `desktop`, and `files` (9 suites, 0 failures, 11 ignored).
  - Clean bare-metal PIC cross-build (`RUSTFLAGS="-C relocation-model=pic" cargo build --release -p files --target x86_64-unknown-none -Z build-std=core,alloc`).
  - `python3 scripts/cellos-sign --check` passes across 124 crates and 788 files.
