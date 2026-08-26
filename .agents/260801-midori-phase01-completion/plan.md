---
title: "Midori Phase 01 Partial Closure Plan"
description: "Add honest QEMU evidence for ReadFileGrant and record unresolved ReadGrant/fast-IPC blockers without overclaiming completion."
status: partial
priority: P1
effort: 18h
branch: main
tags: [bugfix, kernel, vfs, testing, critical]
blockedBy: []
blocks: [phase-01-p02-runtime-verify-fast-IPC-gap]
created: 2026-08-01
---

# Midori Phase 01 Partial Closure Plan

## Overview

Tighten Phase 01 evidence without overclaiming completion: add QEMU-visible `ReadFileGrant` allow/deny coverage, verify whether `ReadGrant` has a real source, and keep original Phase 01 partial because fast-IPC `GetFile` remains blocked by D1.

## Scope Contract

- Deliver: new `ReadFileGrant` test-hooks markers, updated QEMU gate, explicit `ReadGrant` source decision, and updated original evidence/status.
- Exclude: synthetic kernel fast-IPC completion evidence, restoring `resolve_export`/`R_RISCV_JUMP_SLOT`, loader import bridging, `libs/api` or public syscall changes, Tier-1 rewrite, `GetFile`/`DataPtr` production promotion.
- Preserve: Spec 17 remains model of record; direct dispatch remains non-production today.

## Data Flow

- `vfs-test` allocates and shares grants, sends ordinary VFS IPC, VFS authorizes/copies, serial markers exit through QEMU.
- `ReadGrant` source verification enumerates `HandleTable::insert_ro` callers and either proves a real source or records the blocker.
- Fast-IPC remains outside this plan's implementation data flow; its evidence requires separate approved Tier-1 rewrite/rescope.

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 1 | [ReadFileGrant QEMU Evidence](./phase-01-readfilegrant-qemu-evidence.md) | completed | none |
| 2 | [Conditional ReadGrant Source](./phase-02-conditional-readgrant-source.md) | blocked | 1 |
| 3 | [QEMU Evidence and Status Closure](./phase-03-qemu-evidence-and-status-closure.md) | completed | 1, 2 if unblocked |

## Dependencies

- Phase 2 is source verification first: if no real internal handle source exists, mark `ReadGrant` as a contract blocker and continue Phase 3 with original Phase 01 partial.
- No parallel phase ownership: phases are sequential because Phase 3 consumes markers from Phase 1 and the Phase 2 decision.
- Fast-IPC `GetFile` is blocked by D1 and requires a separate explicitly approved Tier-1 rewrite/rescope.

## Baseline Commands

- `bash scripts/build-test-hooks-ci.sh`
- `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test vfs-quota riscv64_vfs_quota_all_pass -- --nocapture`
- `rg -n "resolve_export|R_RISCV_JUMP_SLOT" kernel/src docs/specs/17-ipc-wire-contract.md`

## Validation Log

### Verification Results

- Tier: Standard
- Claims checked: 12
- Verified: 12 | Failed: 0 | Unverified: 0

### Red Team Review

- Accepted: synthetic kernel fast-IPC selftest is invalid completion evidence. Mitigation: remove it from plan and keep Phase 01 partial.
- Accepted: fake `ReadGrant` coverage risk. Mitigation: Phase 2 blocks unless `HandleTable::insert_ro` has a real internal source.
- Accepted: status-overclaim risk. Mitigation: Phase 3 must keep original Phase 01 partial unless separate fast-IPC rewrite/rescope exists.

## Handoff

Next: `$hc-cook .agents/260801-midori-phase01-completion --auto`
