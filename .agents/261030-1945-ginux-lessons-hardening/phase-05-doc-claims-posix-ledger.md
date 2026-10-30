---
phase: 5
title: "Semantic documentation consistency and optional POSIX ledger"
status: pending
priority: P2
dependencies: [1]
tier: medium
---

# Phase 05: Semantic documentation consistency and optional POSIX ledger

## Overview

Apply item 6 through structured ownership/status and callable compatibility contracts. Do not copy Ginux's prose `require_text`/`reject_text` tests: exact wording is not architecture enforcement and would pin stale prose.

## Requirements and architecture

- Extend existing spec-anchor/status tooling where possible. Check canonical decision references, acceptance ownership, dependency/link validity and contradictory structured statuses. Prose remains review input, not source-text unit tests.
- A completed planning deliverable is not equivalent to all runtime ACs qualified. Keep implementation, planning, runtime evidence and physical/production admission as distinct fields/references; derive or flag conflicts, never auto-upgrade status.
- POSIX is an optional personality/runtime contract under ADR-0018, not a native SAS ABI and not a kernel-wide syscall coverage target.
- Ledger per adapter/profile: consumer-visible POSIX entry point, native backing service or mlibc sysdep, supported semantic subset, deterministic unsupported result, test/evidence reference. Separate helper/sysdep names from libc symbols; no misleading one-to-one mapping.
- Status Complete requires behavioral evidence, not only symbol presence; discovery cross-check finds unlisted exports but never proves conformance. Preserve ENOSYS/explicit policy refusal where specified. No kernel POSIX implementation or mandatory POSIX linkage for native Cells.

## Related Files

Reuse `scripts/check-spec-anchors.py` and current CI placement. Proposed ledger `docs/posix-ledger.md`, discovery/check tooling only if the existing checker cannot represent it. Link from ADR-0018 and Spec 05; do not alter their accepted scope.

## Implementation Steps

1. Model structured consistency checks using existing plan/spec metadata; flag README/portfolio disagreement for evidence reconciliation without pretending all statuses share one meaning.
2. Enumerate real adapters/exports/sysdeps and map them to consumer-visible behavior. Unsupported functions stay explicitly partial/missing/denied per profile.
3. Exercise a native build with POSIX omitted and an existing compatible POSIX workload with the adapter enabled. Test a supported semantic case and a documented unsupported operation; choose existing runners after current-source verification.
4. Add CI discovery/link/status checks. No wording snapshots or historical-comment escape hatches.

## Success Criteria

- [ ] Unresolved structured status/owner/link inconsistency is diagnosed, not silently rewritten.
- [ ] Ledger distinguishes native transport, POSIX adapters and Linux VM; libc symbol vs sysdep mapping is explicit.
- [ ] Native SAS runtime remains independent of POSIX. Supported/unsupported adapter behavior is observed through actual consumers.
- [ ] Discovery rejects an unlisted export and a malformed ledger entry without claiming full Linux/POSIX conformance.

## Assumptions

Existing checker extensibility and actual adapter export forms require reconnaissance; no new parser convention is assumed.

## Security and risk assessment

Names and compatibility status confer no authority. No broad text-scanning test should become an implementation oracle. Revert tooling/docs; do not reset preserved runtime evidence.

## Deviation Log

SAS review: replaced literal prose checks; removed accidental Linux-first framing and mandatory native compatibility overhead.
