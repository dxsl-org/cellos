---
title: "Midori Closure Execution Plan"
description: "Close the current ADR/docs, integrate pending Midori slices, and amend Phase 02 runtime criteria before continuing 04/07/08."
status: completed
priority: P1
effort: 9d
branch: main
tags: [feature, docs, critical]
blockedBy: []
blocks: [260727-2101-midori-lessons-cellos]
created: 2026-08-05
---

# Midori Closure Execution Plan

## Overview

Execute the approved 1-4 sequence as one WIP-limited closure stream for the sole active feature program. No new queued program starts until Midori phases 02/04/07/08 are honestly closed.

## Progress

- Phase 01: completed.
- Phase 02: completed.
- Phase 03: completed.
- Phase 04: completed (respawn proof deferred; kernel launch-edge authority landed instead of an ambient spawn broker).
- Phase 05: completed on 2026-08-06 as an honest Phase 07 closure; status is now verified NET_RX-only substrate with generic reactor, peer-death CQ, async VFS/DMA, and executor work still deferred.
- Phase 06: completed on 2026-08-06 as the Phase 08 stack-sizing baseline gate; default 64-page stacks remain unchanged, baseline markers pass on RV64 test-hooks, production stack shrink remains blocked on parked-executor or equivalent post-shim measurements, and the original Phase 08 stays partial/open.

## Evidence Baseline

- Active portfolio says `260727-2101-midori-lessons-cellos` is the sole active program and exits only after runtime-close 02 plus completion of 04/07/08 (`.agents/plan-portfolio.md:8-12`).
- Current branch is `main`; `git status --short --branch` is `main...origin/main [ahead 7]` with unrelated shared-tree changes still present, and the Phase 02-specific rustfmt-only diff is captured in `.agents/reports/review-decision-phase02-integration-260805-200846.json`.
- `git log --oneline --decorate -12` shows `3bd8aaf0` and `3f6ad45d` on `main`; those are the landed evidence for the pending closure slice.
- Phase 08 baseline evidence: RV64 test-hooks markers pass for init/shell/vfs/vfs-test; default 64 pages stay in force and the numbers are not treated as production sizing input.
- Law 1 covers `libs/api/` and `libs/types/` and requires 2 explicit confirmations (`docs/code-standards.md:12-18`).
- Phase 04 must not add broker service ID 13 or public SpawnBroker request/response ABI unless the user explicitly overrides the kernel launch-edge design after Law 1 confirmation.
- The stack-only `GrantSlice` contract is Law1 2/2; Phase 03 no longer owns implementation of `ReadGrant` producer or fast-IPC direct dispatch because both require separate design prerequisites.
- `ReadGrant` producer evidence: VFS `HandleTable::insert_ro` is service-local (`cells/services/vfs/src/handle_table.rs:56-65`) and currently exercised only by tests (`cells/services/vfs/src/handle_table.rs:134-136`), while production `OpenCap`/`ReadCap`/`CloseCap` lives in kernel `CAP_TABLE` (`kernel/src/task/syscall.rs:2764-2808`, `kernel/src/task/syscall.rs:3057-3071`, `kernel/src/cell/cap_registry.rs:257`); the `GetFile` proof here is metadata-only response proof, not raw-pointer dereference.
- Fast-IPC evidence: kernel-side `call_vfs` documents the D1/Spec17 relocation gap and says cell-local direct calls read their own zero handler pointer/fallback, so direct fast `GetFile` cannot be a Phase 02 positive runtime proof (`kernel/src/fast_ipc.rs:121-135`).
- Tier evidence: Spec 18 says `DataPtr`-style raw pointers are unrepresentable across Tier-2 boundaries (`docs/specs/18-cell-trust-tiers.md:151-156`).
- Done requires QEMU evidence and same-commit status text, not checkbox-only closure (`docs/code-standards.md:270-291`).

## Phases

| Phase | Name | Status | Depends |
|-------|------|--------|---------|
| 01 | [Close ADR documentation](./phase-01-close-adr-documentation.md) | completed | - |
| 02 | [Integrate pending closure commits](./phase-02-integrate-pending-closure-commits.md) | completed | 01 |
| 03 | [Amend Phase 02 runtime closure criteria](./phase-03-runtime-close-phase-02.md) | completed | 02 |
| 04 | [Complete Phase 04 deprivilege](./phase-04-complete-deprivilege.md) | completed (respawn proof deferred) | 02 |
| 05 | [Close Phase 07 reactor honestly](./phase-05-complete-reactor.md) | completed | 03, 04 |
| 06 | [Prepare Phase 08 stack sizing gate](./phase-06-complete-stack-sizing.md) | completed (gate only; original Phase 08 partial/open) | 05 |

## Dependency Graph

`01 -> 02 -> 03 -> 04 -> 05 -> 06`; Phase 04 may continue only after Phase 03's status rule is resolved, so later work does not inherit a false-green Phase 02. Phase 05 may not start peer-death/event-bit/API work before Law 1 confirmation #1 and #2. Phase 06 may prepare default-only stack-sizing plumbing and test-hooks baseline markers, but watermark-driven production sizing remains blocked until a real parked executor or equivalent post-shim measurement path exists; do not assume a generic reactor, async VFS/DMA, or peer-death CQ from the current Phase 05 closure.

## Checkpoints

- After Phase 01: user confirms commit/merge direction for docs if conflicts appear.
- Phase 03 approval checkpoint: satisfied on 2026-08-05. Original Midori Phase 02 is now runtime-closed under the approved amended criteria only.
- Before Phase 04 public API work: stop for Law 1 confirmation #1 and #2. Current Phase 04 plan avoids public API/service-ID work and uses kernel-internal launch-edge profiles.
- Before Phase 05 peer-death CQ or new event/API surface work: Law 1 confirmation #1 and #2.
- Before marking any phase complete: runtime evidence + docs/status update in the same change.

## Handoff

Run implementation via `$hc-cook .agents/260805-1833-midori-closure-execution/plan.md`.

## Unresolved Questions

- `docs/coding.md` and `docs/engineering-standards.md` were requested by injected instructions but are absent in this repo; this plan uses `docs/code-standards.md`.
- Exact remote/push/PR policy for integrating `b5a97125` and `eecfbb72` is not chosen here; Phase 02 requires a pre-flight check before any destructive branch cleanup.
