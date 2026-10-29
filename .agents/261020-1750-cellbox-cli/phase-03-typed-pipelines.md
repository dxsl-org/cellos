---
phase: 3
title: "Typed native pipelines and explicit external conversions"
status: completed
priority: P1
tier: thinking
dependencies: [1, 2]
---
# Phase 03: Typed command data

## Overview and requirements

Replace text-capture composition with the accepted typed/native and raw-byte boundaries. Deliver CBX-06..08 without importing Nushell's whole language or assuming a shared executable shares objects between Cells.

## Architecture and related files

Modify `cells/tools/shell/src/{parser,executor,shell_state}.rs` and appropriate pure code in `libs/text-engine`. Stage portable command data/catalog code for migration into `cells/tools/cellbox` in P04; use a small independent pure crate only if actual reuse/test boundaries justify it. Consume Terminal SDK for external byte streams; keep external framing out of kernel authority schemas unless specifically approved.

## Implementation steps

1. Implement the P01-frozen Value/record/record-stream/byte-stream distinction and command input/output signatures. Keep terminal interaction a separate session contract, not a serialize-everything value kind.
2. Implement catalog validation and the bounded consumer-driven native pipeline engine. Move/borrow values directly, avoid an intermediate formatted String or JSON parse per stage, and preserve integer/unit/absence semantics.
3. Add only field selection, typed comparison, finite sorting/take and explicit JSON/JSONL/text conversion needed by selected consumers. Preserve declared input nesting and stream ownership; no clone-that-drains-another-stream behavior.
4. Implement exact byte external adapters using P02, separate diagnostics/status, partial failures, cancellation and EOF. Require explicit record-to-byte conversion; never send a table or guess a remote JSON format.
5. Define record redirection/non-terminal encoding exactly as frozen in P01. Encode wide IDs and non-UTF-8 data losslessly; reject malformed/truncated input and ambiguous type conversions before effects.
6. Retain the P01-frozen standalone/pipeline stateful-builtin context and script semantics. Give supported mixed native/external pipelines one job identity with defined status, foreground transfer, wait/reap and cancellation; unsupported background/stateful compositions are rejected before effects. Migrate changed scripts/tests/docs together, never silently ignore `&` or mutate the parent's cwd/environment from an unintended context.
7. Prove real finite record and byte/external pipelines, early consumer stop and bounded frames. Exercise supported background mixed-pipeline `jobs`/`fg`/`bg` transitions or the exact pre-effect rejection for unsupported forms; verify parent state, status and foreground input recover. Remove throwaway fixtures after proof.

## Success criteria

- [x] Numeric filtering/sorting yields correct values without parsing display units; `null` remains distinct from zero.
- [x] JSON/JSONL/bytes preserve boundaries and values; bad types/encodings fail truthfully and do not trigger unintended mutation.
- [x] Byte payloads and stderr are not injected into success records; status and cancellation propagate under the declared policy.
- [x] Continuous frames begin promptly, stay bounded and stop when the consumer closes; global sort never waits on an infinite history.

## Verification

Retain deterministic tests for type precedence, conversion/error boundaries, precision and cancellation where plausible consumer bugs exist. Exercise actual shell pipelines on signed x86; a function-only test or echoing a mocked record is not end-to-end proof. Do not pin pretty-print wording or source layout.

## Assumptions

Chosen parser syntax can be added without a second evaluation language and current scripts have an enumerable migration surface. Verify full caller/script inventory at entry; escalate any required language expansion rather than inventing a Nushell clone.

## Security and risk assessment

Untrusted records cannot instantiate handles or select privilege. Bound nesting, record/frame size and retained sort input. Rollback needs matched parser/executor/scripts and image; the old and new grammars cannot remain hidden fallback modes. Already-written output/files are not undone by rollback.

## Deviation log

2026-10-20 — **Completion withdrawn; incomplete/blocked.** Pipeline model/validation/conversion source was added in `libs/text-engine::pipeline`; the historical 42-host-test snapshot is preserved as unit evidence only. It is not end-to-end proof of the real shell parser/executor, mixed external/native jobs, incremental bounded producers or cancellation/status propagation. Review found no-op transform paths in cellbox and an exiting shell; no actual signed-image pipeline proof establishes CBX-06..08. Design consent remains recorded, full source-admission inputs are incomplete, and CP2 is pending/not eligible. All criteria remain unchecked; rollout is blocked under [reports/rollout-containment.md](reports/rollout-containment.md).

2026-10-21 — Admitted after Phase 02 CP2. Implemented typed engine, shell typed runtime, native producers and cellbox transforms; private image 15/15, host 74+44. **Incomplete:** external byte adapters, mixed jobs, in-VM scenarios, cellbox image execution. Success criteria stay unchecked until those are closed. See [evidence](reports/phase-03-runtime-evidence.md).

2026-10-21 — A3 (external byte stages) and A4 (preparer-queryable child status, private filter fixture) implemented; private image 39/39. Criteria checked on private-image evidence only. Pending: Phase 03 CP2, in-VM `shell_test`, cellbox standalone execution; rollout stays quarantined.

2026-10-21 — **CP2 approved by the user (“Duyệt CP2 Phase 03 để mở Phase 04”)** as the Phase 03 implementation baseline (private-image evidence, A3+A4 closures). Does not lift the rollout quarantine, canonical packaging/signing, or approve Phase 04 source/launch-profile/image edits.
