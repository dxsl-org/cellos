---
phase: 2
title: "CellOS Linear shared tokens, components and live appearance"
status: pending
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
- [ ] All three palettes share geometry; normal text contrast >=4.5:1 and focus/status remain distinguishable without color alone.
- [ ] Running consumers refresh appearance without losing view state; missed update/restart resynchronizes, malformed updates cannot crash UI.
- [ ] No source-text/copy tests: permanent tests exercise contrast boundaries, preference precedence and stale revision handling; live captures prove actual rendering.
- [ ] No new independent Ocel reader palette; chrome follows system while explicit Reader choice remains fixed.

## Security Considerations
An untrusted app cannot forge system theme changes or mutate another user's/session's preferences. No dynamic executable theme plugins or remote assets; bound wallpaper/palette inputs.

## Risk Assessment
Risk: renderer/token conversion changes painting or contrast. Roll back theme consumer/provider patch together to the pinned image; do not keep deprecated token adapters as a permanent second convention. Saved preference values may remain; version them and document unsupported-old-reader behavior. No arbitrary migration of user files.

## Deviation Log
Append during execution; do not reconstruct after shipping.
