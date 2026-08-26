---
title: "Part 6 Blocking Decision Closure"
description: "Plan to close D1 consequents, D1b IPC target semantics, D3 kernel LOC ownership, and D5 cell-scale profile."
status: complete
priority: P1
effort: 9h
branch: feat/wx-post-reloc-and-f1-signing
tags: [docs, critical, architecture]
blockedBy: []
blocks: [midori-runtime-closure, spec21-status-generation]
created: 2026-08-01
---

# Part 6 Blocking Decision Closure

## Overview

Close the remaining blocking architecture decisions that define Cellos's IPC performance story, kernel-size reporting contract, and server-scale cell target. This is an architecture/docs plan only unless Phase 2's scaffold audit finds dead code safe to delete.

## Phases

| Phase | Name | Status |
|---|---|---|
| 1 | [Define IPC Benchmark Semantics](./phase-01-ipc-benchmark-semantics.md) | complete |
| 2 | [Reconcile Fast-IPC Consequents](./phase-02-fast-ipc-consequents.md) | complete |
| 3 | [Move Kernel LOC To Generated Status](./phase-03-kernel-loc-status.md) | complete |
| 4 | [Lock Per-Request Cell Scale Profile](./phase-04-cell-scale-profile.md) | complete |

## Dependencies

- Phase 2 depends on Phase 1 because the fast-IPC prose must cite the same benchmark semantics as the bench gate.
- Phase 3 is independent, but should land before Phase 4 if the scale profile cites the generated status layer.
- Phase 4 depends on A1/A2/A3 already closed: DTB memory discovery, typed spawn OOM, and MemInfo are prerequisites for honest scale measurement.

## Data Flows

- Benchmark data enters `cells/tests/bench`, is reduced to p99, then exits as CI pass/fail and docs evidence.
- Kernel LOC data enters from `kernel/src`, is transformed by a generated status command, then exits as status evidence that specs cite without freezing numbers.
- Cell-scale evidence enters from loader/quota/memory accounting, transforms into profile-specific gates, then exits as Spec 19/PDR roadmap wording.

## File Ownership

- Phase 1 owns `cells/tests/bench/src/main.rs`, `docs/performance-report.md`, and `docs/project-overview-pdr.md` performance wording.
- Phase 2 owns fast-IPC wording in `docs/specs/00-context.md`, `docs/specs/16-rustc-tcb.md`, `docs/specs/17-ipc-wire-contract.md`, plus any confirmed dead scaffold in `kernel/src/fast_ipc.rs`, `kernel/src/loader/reloc.rs`, and `libs/ostd/src/fast_ipc.rs`.
- Phase 3 owns LOC wording in `docs/specs/00-context.md`, `docs/specs/12-reliability.md`, `docs/specs/15-kernel-boundary.md`, `docs/specs/16-rustc-tcb.md`, `docs/system-architecture.md`, and `docs/project-overview-pdr.md`.
- Phase 4 owns scale wording in `docs/specs/19-hardware-isolation-layers.md`, `docs/project-overview-pdr.md`, `docs/project-roadmap.md`, and `docs/TODO.md`.

## Closure

- D1b makes 50 us p99 a qualified-hardware target; scheduled QEMU owns sustained regression evidence.
- D1 removes unreferenced export/JUMP_SLOT claims and marks retained handler tables inactive.
- D3 generates and CI-checks total/core nLOC instead of freezing prose totals.
- D5 queues the accepted 1000-cell per-request profile behind Midori; current defaults are unchanged.
- Focused cargo checks, generated-metric checks, `git diff --check`, tester, and reviewer passed.
