---
title: "Phase 07 Atomic Task Publication Prerequisite"
description: "Resolve CELLOS-LOADER-RACE-002 and CELLOS-LOADER-CLEANUP-003 without enabling Tier 2 or changing the signature boundary."
status: completed
completion_state: "ATOMIC_PUBLICATION_PREREQUISITE_COMPLETE / PHASE07_BLOCKED"
verification_status: verified
priority: P1
effort: 3d
branch: main
tags: [bugfix, backend, critical]
blockedBy: [260821-0642-app-tiers-completion/phase-03-tier1-baseline-admission, 260821-0642-app-tiers-completion/phase-04-tier3-qualification]
handoff: "Atomic publication is complete; full Phase 07 returns to its Phase 03/04 and Tier 2 qualification gates. Phase 08 remains directly blocked on Phase 03, Phase 05, and full Phase 07."
created: 2026-08-22
---

# Phase 07 Atomic Task Publication Prerequisite

## Contract

Governed spawn MUST finish every fallible admission decision and build a complete task before one final scheduler commit makes it ready. A denied spawn MUST leave task tables, task IDs, ready queues, quota, mappings/frames/PIE slots/stacks, singleton reservations, service routes, measurement-success state, and replacement authority exactly as they were before the attempt. Denial audit records and documented one-shot syscall input consumption are not successful-launch state. Trusted embedded init keeps its deliberate signature/manifest bypass, but its boot ceiling, `SupervisorCap`, critical flag, identity, context, and inherited state MUST be installed before publication.

`CELLOS-LOADER-SIG-001` remains owned by Phase 03. This child MUST NOT add `AddressSpace`, native-domain code/features/admission, Manifest v3 bytes, execution-tier metadata, or change signature extraction/coverage/verification. Public syscall numbers and argument layouts remain unchanged.

## Architecture

`spawn entry → immutable preflight decision → RAII PreparedElfTask + reservations → infallible scheduler commit (configure → insert → global success side effects → ready queue LAST) → tid`

The commit is the only successful publication point. No `Result`-returning callback, policy lookup, ELF parse, allocation, quota denial, singleton acquisition, replacement validation, or route validation may run after it begins. Success measurement/audit are staged and committed only after the last injectable/fallible point and before the ready push.

## Caller Inventory / Clean Cutover

- Governed core: `loader::spawn_from_path`, `loader::spawn_gated`, and `loader::mem_spawn_gate::spawn_from_mem_gated` adopt one `SpawnRequest`; no parallel legacy signature.
- Syscall routes: `SpawnFromPath`, `SpawnFromElf`, `SpawnFromMem`, `SpawnPinned`, and `SpawnReplacement` provide route options before loading. Priority/cluster denial, argv/dir handoff, and replacement binding move before ready publication.
- Boot: RISC-V Platform remains governed through `spawn_from_path`; embedded init migrates to the explicit trusted-complete API.
- Task internals: remove raw `task::spawn_from_mem`; migrate both direct callers and stale comments. Do not leave a wrapper, alias, deprecated re-export, or default config that can publish an incompletely configured task.
- Tests/docs: extend the boot-time loader corpus, update W^X/publication text and changelog, then sync the umbrella risk ledger while leaving umbrella Phase 07 blocked.

## Phases

| ID | Phase | Effort | Depends on | Status |
|---|---|---:|---|---|
| 01 | [Prepare owned task and rollback primitives](phase-01-prepared-task-and-reservations.md) | 1d | — | completed |
| 02 | [Cut over publication and every caller](phase-02-atomic-commit-and-callers.md) | 1d | 01 | completed |
| 03 | [Inject failures and prove restoration](phase-03-failure-proof-and-ledger-sync.md) | 1d | 01,02 | completed |

## Program Gate

Completion resolves only `CELLOS-LOADER-RACE-002` and `CELLOS-LOADER-CLEANUP-003`. The required terminal is exactly:

`ATOMIC_PUBLICATION_PREREQUISITE_COMPLETE / PHASE07_BLOCKED`

Phase 07 remains blocked on Phases 03 and 04 and cannot expose Tier 2. Phase 08 remains blocked on Phase 07.

## Verification record

The atomic prerequisite is verified, not a full Phase 07 qualification or release/approval result.

- A fresh test-hooks build and signing pass completed.
- The populated-fixture one-hart VFS run passed `1/1`; applicable atomic cases `AP-00` through `AP-11` and `AP-15` passed, while the two-hart-only `AP-13` emitted `SKIP`.
- The distinct `-smp 2` atomic run passed `1/1`: `AP-00` through `AP-15` passed, including `AP-02` live PTE/raw-flags then post-drop no-translation/no-frame/no-TLB-visible-mapping proof and the `AP-13` remote-hart scheduler-attempt witness. It emitted `ATOMIC_PUBLICATION_PREREQUISITE_COMPLETE / PHASE07_BLOCKED` and `ATOMIC_PUBLICATION_ALL: PASS`.

This closes only the atomic-publication prerequisite. It does not qualify Tier 2, close Phase 03 provenance/signature work, complete Phase 04, or grant any security, release, ledger, or human approval.

## Separate SMP VFS release blocker and handoff

The unbaselined two-hart VFS result remains a separate release blocker: after the atomic terminal markers, VFS reported `40 PASS, 10 FAIL`. The probable cause is the pre-existing wildcard VFS RPC receive, not a claimed Phase 07 regression; no pre-Phase07 two-hart VFS baseline exists. Evidence and required regression are in [`debug-260822-0749-smp-vfs.md`](../debug/debug-260822-0749-smp-vfs.md).

**Next owner:** the separate VFS SMP repair workstream, specifically the VFS test/client request-reply transport owner. It must make the receive sender-aware and establish the required two-hart VFS regression; this work is not part of the verified atomic prerequisite.
