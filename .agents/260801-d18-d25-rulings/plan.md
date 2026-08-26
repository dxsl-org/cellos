---
title: "D18-D25 Rulings Implementation Plan"
description: "Apply recommendation A for D18-D25 as documentation/docket updates plus one dead ostd wrapper deletion."
status: complete
priority: P2
effort: 3h
branch: feat/wx-post-reloc-and-f1-signing
tags: [docs, tech-debt]
blockedBy: []
blocks: []
created: 2026-08-01
---

# D18-D25 Rulings Implementation Plan

## Overview

Apply the authorized D18-D25 recommendation-A rulings with no runtime behavior change: update docs/docket/report status, delete only the unused `libs/ostd/src/syscall.rs::sys_grant` wrapper, preserve existing D15-D17 dirty worktree edits, and leave `libs/api`/`libs/types` untouched.

## Phase 1 - Docs And Docket Rulings

Owner: docs implementor. Files: `.agents/reports/decision-docket-260730.md`, `.agents/reports/d18-*` through `.agents/reports/d25-*`, `docs/project-roadmap.md`, `docs/project-changelog.md`, `docs/system-architecture.md`, `docs/README.md`, and affected specs under `docs/specs/`.

Data flow: D18-D25 ruling reports enter as authoritative decision inputs; docs/docket text is transformed from unresolved/stale claims into ruled status and current invariants; outputs are documentation-only statements. Key anchors: D18 withdraws monolithic Metadata Registry (`.agents/reports/d18-metadata-registry-ownership-analysis-260801.md:25`); D19 replaces `catch_unwind` with terminate-and-supervise (`.agents/reports/d19-panic-recovery-contract-analysis-260801.md:22`); D20 recognizes active Grant API (`.agents/reports/d20-grant-api-reachability-analysis-260801.md:19`); D21 defers Layer-B ADR and makes `GetFile/DataPtr` removal a Tier-2 prerequisite (`.agents/reports/d21-layer-b-adr-getfile-prerequisite-analysis-260801.md:25`); D22 updates VFS/watchdog/work-stealing ownership (`.agents/reports/d22-kernel-boundary-mechanisms-analysis-260801.md:24`); D23 splits RV64 dev from ARM64 qualification (`.agents/reports/d23-production-certification-lane-analysis-260801.md:24`); D24 keeps Spec 20 Draft with zero ABI (`.agents/reports/d24-spec20-ratification-order-analysis-260801.md:17`); D25 adds NodeId-derived `machine_id` invariant (`.agents/reports/d25-machine-id-binding-analysis-260801.md:19`).

Acceptance criteria:
- [x] D18-D25 docket/report statuses are marked ruled/applied using the existing D15-D17 closed style.
- [x] Docs no longer make stale normative claims for Metadata Registry, `catch_unwind`, Resource-Graph deadlock diagnosis, Spec 20 ratification, or peer-authored `machine_id`.
- [x] Spec 20 remains Draft and explicitly approves zero Law-1 ABI additions.
- [x] D21 states detailed Layer-B ADR and public API mutation are deferred to a separate Law-1 package.

Risk and rollback: Medium x High risk of overwriting dirty D15-D17 doc edits; mitigate with line-local edits and `git diff` review before/after. Roll back by reverting only Phase 1 hunks; whole-section rewrites are forbidden because they can erase existing pending work.

## Phase 2 - Dead Wrapper Cleanup

Owner: Rust implementor. File: `libs/ostd/src/syscall.rs` only.

Data flow: the uncalled `sys_grant` stub enters as dead API surface; deletion removes the misleading wrapper; active grant wrappers remain unchanged. Verified boundary: only in-repo match is `libs/ostd/src/syscall.rs:962`; active wrappers use `GrantAlloc/Share/Slice/Free/Register/Unregister` at `libs/ostd/src/syscall.rs:1301`, `:1321`, `:1337`, `:1373`, `:1389`, `:1403`. ABI definitions in `libs/api/src/abi/syscall.rs:222`, `:226`, `:231`, `:234`, `:253`, `:257` are out of scope.

Acceptance criteria:
- [x] `rg -n "sys_grant\\(" libs cells kernel tests -g "*.rs"` returns no matches.
- [x] `git diff -- libs/ostd/src/syscall.rs` shows only the legacy wrapper deletion.
- [x] No `libs/api/` or `libs/types/` files are modified.

Risk and rollback: Low x Medium downstream-API risk if external code imported the dead wrapper; D20 rules it is not ABI. Roll back by restoring the deleted wrapper block; no irreversible part.

## Phase 3 - Verify Review

Owner: tester/reviewer. Files: read/verify only except fixes within Phase 1/2 ownership.

Data flow: final diff and command output enter verification; grep/compile/review transform them into pass/fail evidence; output is a concise completion report.

Acceptance criteria:
- [x] `git diff --name-only` contains only the cumulative D15-D33 owned files.
- [x] `cargo check -p ostd` and focused affected-package checks pass in WSL.
- [x] Stale-claim grep for D18-D25 terms confirms docs are consistent.
- [x] Review confirms D24 zero ABI, D21 ADR deferred, D25 docs invariant only, and D15-D17 dirty worktree changes preserved.

Risk and rollback: Medium x Medium compile may be blocked by unrelated dirty state; mitigate by using the narrowest supported check and reporting only decisive evidence. Roll back any verification fixes through their owning phase hunks.

## Non-Goals

- No `libs/api` or `libs/types` edits; no Law-1 ABI proposal.
- No runtime behavior, kernel dispatch, wire-format, remote IPC, enrollment, or page-table-domain work.
- D21 Layer-B ADR remains deferred; D24 Spec 20 remains Draft and approves zero ABI; D25 is docs invariant only.

## Dependencies

- Phase 1 and Phase 2 may run independently because they own disjoint files.
- Phase 3 depends on Phase 1 and Phase 2.
- Existing dirty worktree files from D15-D17 and pending service/kernel edits must be preserved; do not revert unrelated changes.

## Unresolved Questions

- None for this scoped pass. A later Law-1 package must separately ask for two confirmations before any ABI layout, discriminant, or public type change.
