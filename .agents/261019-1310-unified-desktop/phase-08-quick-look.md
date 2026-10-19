---
phase: 8
title: "Bounded shared preview and Files Quick Look"
status: pending
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
- [ ] Four requested format families preview in Files with no spawned viewer tab; Open With creates real associated app session.
- [ ] Late/cancelled result cannot replace current preview, block navigation or reuse in-flight grant memory; all quotas enforced.
- [ ] Shared Ocel/PDF use survives busy, error, service death and concurrent clients without cross-owner content leakage.
- [ ] An intentionally non-terminating render worker hits its watchdog, reaches confirmed generation death and releases quarantined resources; unrelated previews subsequently progress without unbounded restart loops.
- [ ] Actual visible output and focus tested in three palettes, raw image/PDF colors retained, measured latency labels truthful.

## Security Considerations
Untrusted files must not execute scripts/URLs, exceed finite decode budgets or gain directory authority. Reuse shared decoder code without moving unsafe FFI into trusted Files. New isolated services need reviewed manifests/signing and real demand lifecycle.

## Risk Assessment
Decoder crash or stuck C call is a real boundary. Roll back shared extraction and both consumers together; no permanent fork or fake preview fallback. In-flight jobs must be drained/terminated safely before buffer reuse; temporary decoded caches may be discarded, user source files are never mutated.

## Deviation Log
Append during execution.
