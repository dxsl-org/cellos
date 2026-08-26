---
phase: 1
title: "Phase 02 Runtime Verification Closure"
status: partial
priority: P1
effort: "1d"
dependencies: []
tier: fast
---

# Phase 01: Phase 02 Runtime Verification Closure

## Overview

Close Midori Phase 01's remaining honest runtime-evidence gap without altering VFS policy semantics. This stream proves existing read gating, owner checks, and `ReadFileGrant` authorization under QEMU while keeping the phase partial because actual fast-IPC `GetFile` and `ReadGrant` remain blocked.

## Requirements

- Functional: Produce boot/runtime evidence for all seven read-gated VFS operations, owner checks, and `ReadFileGrant`, then record the `ReadGrant` source decision explicitly.
- Non-functional: Verification-only unless a test harness bug blocks evidence; do not change VFS authorization behavior.

## Architecture

Data flow: integration harness boots a test-hooks image, `init` spawns `/bin/vfs-test`, `vfs-test` sends typed VFS IPC, VFS derives caller identity and applies `can_read`, then serial output becomes pass/fail evidence.

Observed anchors:
- VFS denies unattested callers at entry: `cells/services/vfs/src/dispatch.rs:28`.
- Read gates cover `GetFile`, `ListDir`, `Stat`, `ReadAsync`, `Poll`, and grants: `cells/services/vfs/src/dispatch.rs:55`, `cells/services/vfs/src/dispatch.rs:72`, `cells/services/vfs/src/dispatch.rs:81`, `cells/services/vfs/src/dispatch.rs:161`, `cells/services/vfs/src/dispatch.rs:178`, `cells/services/vfs/src/dispatch.rs:209`.
- `ReadFileGrant` is separately authorized through the path-addressed gate in `cells/services/vfs/src/dispatch.rs:283` and `libs/api/src/services/ipc.rs:173`.
- Fast-IPC `GetFile` is separately authorized: `cells/services/vfs/src/main.rs:125`, `cells/services/vfs/src/main.rs:133`.
- Existing integration waits for `[vfs-test] ALL TESTS PASSED`: `tests/integration/tests/vfs-quota.rs`.

## Assumptions

- **Claim:** `vfs-test` already covers cross-cell handle ownership sufficiently.
  **Confidence:** medium
  **How to verify:** inspect `cells/tests/vfs-test/src/main.rs` and add harness-only assertions only if coverage is absent.

## File Ownership

- Owns: `tests/integration/tests/vfs-quota.rs`, `tests/integration/src/lib.rs`, `scripts/build-test-hooks-ci.sh`, runtime evidence report under `.agents/.../reports/`.
- Read-only unless evidence gap found: `cells/tests/vfs-test/src/main.rs`, `cells/services/vfs/src/*`.

## Implementation Steps

1. Create worktree from current branch: `git worktree add .worktrees/midori-p02-runtime-verify -b codex/midori-p02-runtime-verify`.
2. Inspect `vfs-test` coverage for read gates, fast-IPC `GetFile`, pending handle ownership, and grant ownership.
3. Run the existing test-hooks build and QEMU test.
4. If a harness omission blocks proving a requirement, add the smallest test-only assertion and rerun.
5. Save concise evidence report with command, commit SHA, serial markers, and unresolved runtime gaps.

## Success Criteria

- [x] Runtime log reaches `[vfs-test] ALL TESTS PASSED`.
- [ ] Evidence names all seven read-gated operations, QEMU-proven `ReadFileGrant`, the blocked `ReadGrant` source decision, and the still-blocked fast-IPC `GetFile` path.
- [x] No product VFS behavior changes unless separately justified as a test blocker.
- [x] Shared integration gate passes or failure is classified as pre-existing with decisive line.

## Security Considerations

Do not relax `/srv` or root read policy to make tests pass. Unknown caller and sealed-directory denials are security invariants.

## Risk Notes

- Risk high impact: runtime failure may reveal a real auth bug. Mitigation: stop and report; do not paper over with harness changes.
- Rollback: drop the worktree branch if only evidence was produced; revert test-only commits if they prove unnecessary.

## Deviation Log

Evidence report: [midori-p02-runtime-verify-report.md](./reports/midori-p02-runtime-verify-report.md).

ReadGrant runtime coverage remains blocked because `HandleTable::insert_ro` has no real source beyond the unit fixture documented in the report.

Fast-IPC proof remains blocked by the guest/runtime lane limitation documented in the report.
