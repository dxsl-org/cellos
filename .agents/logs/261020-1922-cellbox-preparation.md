# 2026-10-20 — Cellbox preparation and retained gates

## What happened
An explicit `hc-cook --auto` plan request entered Phase 01 preparation. Saved current command/caller, contract, dependency and source/test evidence; no source or ABI changes.

## Decisions
- Treat auto mode as workflow-checkpoint resolution, not concrete CP1/CP2 or lock release.
- Keep Phase 01 blocked/incomplete and every CBX pending: inventories are not exact frozen specifications or target proof.
- Preserve pre-existing dirty source/images; no reset, signing, packaging, QEMU, downloads, keys or exposure.

## Lessons
- Existing host text-engine baseline passed 38 unit tests, 0 failed, 0 doctests with offline/locked cargo.
- GUI fixed-response interpreter, shell Vec captures and kernel pipes are separate paths, not an implemented terminal session bridge.
- Historical completion/ratification summaries conflict with narrower corrected approval records; use exact surface evidence.

## Next steps
- Complete frozen command semantics and caller-edge/right denials; exact delta specs/digests and controlling-owner handoffs.
- Select/review upstream revisions/features/licenses/advisories and prove actual admitted target runtime.
- Obtain per-delta CP1 before affected ABI edits; CP2 follows implementation evidence before final retirement.
