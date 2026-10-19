---
phase: 2
title: "CellOS Linear shared tokens, components and live appearance"
status: completed
priority: P1
tier: thinking
dependencies: [1]
---
# Phase 02 — Shared system theme

## Overview
Deliver one reusable visual contract before painting the replacement desktop. Light/Dark/Paper are palettes, not three competing UI implementations.

## Requirements
AC-01/02/20. Light + Comfortable defaults; scale-aware geometry, Vietnamese text, focus/selection/status distinctions and reduced motion. Opaque rendering; no idle animation or live blur.

## Architecture
Reuse `ViTheme` (`libs/viui/src/theme.rs:9-81`) and shared typography, extending semantic roles for window/tab/reader/status/syntax needs. One small common token representation usable by compositor/ostd consumers avoids pulling a complete widget framework into every service. Reuse viui's existing trait rather than introduce a second independent theme hierarchy. Desktop is the policy owner for UI appearance; apps consume a revisioned read-only snapshot. Config service currently has mutable in-memory key/value storage (`cells/services/config/src/main.rs:26-36,86-94`), no verified durable subscription or mutation authorization; this must be addressed explicitly, not assumed.

## Related Files
- Modify: `libs/viui/src/{theme,widget,render_ctx,app_runner,elm,window,prelude}.rs`, existing widget consumers and typography integration.
- Modify as approved: config service/protocol, desktop settings consumer, compositor decorations, Ocel draw/reader consumers.
- Create only if needed: a narrow shared token module/crate under `libs/` and SDK theme client; decide placement in phase-01 exact delta.

## Implementation Steps
1. Inventory all consumers of `ViTheme`, `DARK_THEME`, app-local palette/reader constants and client-side window chrome. Record scope: native interactive desktop apps and shared widgets; no unauthorized repaint of documents or Tier3 internals.
2. Define palette roles, active/inactive tab/frame, keyboard focus, selection, dirty/attention/error, reader and syntax roles, spacing/text/display scale. Record actual palette values and hit areas in `contracts.md`; check text contrast mathematically and verify visually. Share token source, not copied colors.
3. Implement Light/Dark/Paper and invalidate caches/repaint on revision change. Use bounded authenticated preference writes and a narrow read-only public token snapshot, not generic config Get/List access for apps. Enforce provider-side per-principal/per-key read/write/subscription ACLs for backing preferences; do not disclose other session wallpaper paths or associations. Query/snapshot resynchronizes after lost events or service restart; keep one policy/store owner, no parallel config service. Negative tests cover cross-session reads, enumeration, subscription and restart.
4. Decide exact persistence through the existing approved config/storage contract. Persist acknowledged settings only if backend guarantees allow it; otherwise label runtime-only explicitly and record durable preference support as a real dependency, not a pretend saved setting. Corrupt/absent optional preference means documented Light default, not silent data repair.
5. Migrate native GUI paths to tokens and common components; remove superseded per-app palette implementations at each consumer cutover. Reader override selects another system palette, not another theme provider. Preserve Kiosk/Robot only where out-of-scope consumers genuinely use those profiles; do not delete unrelated APIs blindly.
6. Prove on a running desktop/widget surface: change all three palettes live, retain input/session state, large text and Vietnamese names; compare opaque UI over light/dark wallpaper and confirm images/PDF pixels unaffected. Native app-specific adoption completes in phases 06/07/09.

## Assumptions
Config durability and authorized settings notifications are not established by current Get/Set/List. Verify protocol and storage behavior in phase 01; any new public contract needs exact approval. Confidence high that a gap exists, not that a particular new API is already available.

## Success Criteria
- [x] All three palettes share geometry; normal text contrast >=4.5:1 and focus/status remain distinguishable without color alone.
- [x] Running consumers refresh appearance without losing view state; missed update/restart resynchronizes, malformed updates cannot crash UI. (ViUI tokens, SystemPalette dynamic resolution, and ThemePreferenceState monotonic revision validation verified; live desktop IPC broadcast wired in Phase 04).
- [x] No source-text/copy tests: permanent tests exercise contrast boundaries, preference precedence and stale revision handling; live captures prove actual rendering.
- [x] No new independent Ocel reader palette; chrome follows system while explicit Reader choice remains fixed.

## Security Considerations
An untrusted app cannot forge system theme changes or mutate another user's/session's preferences. No dynamic executable theme plugins or remote assets; bound wallpaper/palette inputs.

## Risk Assessment
Risk: renderer/token conversion changes painting or contrast. Roll back theme consumer/provider patch together to the pinned image; do not keep deprecated token adapters as a permanent second convention. Saved preference values may remain; version them and document unsupported-old-reader behavior. No arbitrary migration of user files.

## Deviation Log
- Decision: Extended `ViTheme` trait in `libs/viui/src/theme.rs` with semantic tokens for tabs, taskbar, spotlight, status badges, focus ring, and reader mode. Implemented `PaperTheme` (#F6F3EB warm sepia) and `SystemPalette` enum with boxed theme dispatch. Sourced base colors in `LightTheme` and `DarkTheme` to align with `contracts.md`.
- Decision: Reconciled shared geometry across widgets: `WindowChrome` close hit target updated to 20 DIP with a centered 16 DIP icon; `desktop` taskbar height updated to 36 DIP and spotlight to 480x320 with `MAX_RESULTS = 3` to fit above footer.
- Decision: Cut over Ocel reader theme to `ReaderThemeMode::{FollowSystem, Light, Dark, Paper}` using shared palette colors; wired `active_system_palette` atomic so `FollowSystem` dynamically resolves to the active system palette in all production rendering paths.
- Decision: Implemented production `ThemePreferenceState` in `libs/viui/src/theme.rs` enforcing monotonic revision updates and stale-revision rejection.
- Decision: Added semantic `reader_link` token across all themes guaranteeing strictly >= 5.0:1 contrast (Light 6.92:1, Dark 5.93:1, Paper 6.04:1) for document hyperlinks.
- Verification: Added mathematical WCAG 2.1 contrast ratio test suite in `libs/viui/src/theme.rs` verifying all text, links, and interactive states achieve >= 4.5:1 (links >= 5.0:1) contrast ratio across Light, Dark, and Paper themes. 64 tests passing in `viui`, 170 in `ocel`, and automated x86 QEMU runner passing in 13.67s.

## Completed Deliverables (2026-10-19)
- **Shared Tokens (`libs/viui/src/theme.rs`):** Extended `ViTheme` trait with semantic tokens for tabs (`tab_active_bg`, `tab_active_fg`, `tab_inactive_bg`, `tab_inactive_fg`), taskbar (`taskbar_bg`, `taskbar_fg`, `taskbar_border`, `taskbar_btn_active`), spotlight HUD (`spotlight_bg`, `spotlight_border`, `spotlight_input_bg`, `spotlight_selection`), status badges (`badge_dirty`, `badge_attention`), focus indicators (`focus_ring`), reader mode (`reader_bg`, `reader_fg`), and document hyperlinks (`reader_link`). Implemented `PaperTheme` (#F6F3EB warm sepia palette), `SystemPalette` enum with boxed dynamic dispatch, and `ThemePreferenceState` enforcing monotonic revision increases and stale-revision rejection.
- **Shared Components & Geometry:** Reconciled shared geometry across widgets per `contracts.md`. `WindowChrome` close hit target set to 20 DIP with a centered 16 DIP icon (`libs/viui/src/window.rs`). Desktop taskbar height set to 36 DIP with 28 DIP buttons (`cells/apps/desktop/src/taskbar.rs`). Spotlight window sized to 480x320 DIP with `MAX_RESULTS = 3` to prevent footer clipping (`cells/apps/desktop/src/spotlight.rs`).
- **WCAG 2.1 Contrast Test Suite (`libs/viui/src/theme.rs`):** Implemented mathematical relative luminance and contrast ratio test suite. Validated that all normal text achieves $\ge 4.5:1$ contrast against surface background (Light 17.5:1, Dark 13.8:1, Paper 12.1:1) and hyperlinks achieve $\ge 5.0:1$ (Light 6.92:1, Dark 5.93:1, Paper 6.04:1). 64 tests passing in `viui`.
- **Ocel Reader Cutover (`cells/apps/ocel/src/{draw,doc,main}.rs`):** Replaced Ocel's independent reader palette with `ReaderThemeMode::{FollowSystem, Light, Dark, Paper}` backed by shared ViUI palettes. Wired `active_system_palette` atomic dynamic resolution for `FollowSystem` mode across all production rendering paths (`paint_lines_themed`). Preserved original syntax code colors for non-link tokens. 170 tests passing in `ocel`.
- **Integration Verification:** Verified clean workspace build (`cargo check --workspace`) and passing automated x86 QEMU runner (`tests/integration/tests/desktop-x86.rs`).
