---
title: "SAS/LBI Revocable VFS Access Plan"
description: "Migrate VFS reads away from raw SAS pointers toward bounded, owner-scoped file handles without reopening Midori or Tier 2."
status: in-progress
priority: P1
effort: 9d
branch: main
tags: [feature, backend, critical]
blockedBy: []
blocks: []
created: 2026-08-09
---

# SAS/LBI Revocable VFS Access Plan

## Overview

Preferred target is file handle + bounded read; bounded copy-out remains migration-only. Phases 02, 03, and 04 are complete. Phase 05 caller migration is implemented and under gated verification; retirement remains blocked, and Phase 06 is pending.

## Decision

| Option | Fit | Risk | Decision |
|---|---|---|---|
| A. Bounded copy-out via `ReadFileGrant` / reply `Data` | Best current-code fit; already bounded and live | Still path-addressed; same-SAS tactic only | Use first for migration |
| B. Directory-derived file handle + bounded read | Best endpoint; owner/generation scoped and Tier-2-shaped | New public ABI and durable state | Select as target after Law 1 and cleanup proof |
| C. Revocable `ReadGrant` first | Best eventual perf/revoke story | No production opener; unsafe without pin/ack if cancellable | Defer |

Rationale: `DataPtr` is raw permanent SAS authority (`libs/api/src/services/ipc.rs:197`, `cells/services/vfs/src/dispatch.rs:55`) and Spec 17/18 block it for Tier 2 (`docs/specs/17-ipc-wire-contract.md:449`, `docs/specs/18-cell-trust-tiers.md:155`). `ReadFileGrant` exists now (`libs/api/src/services/ipc.rs:79`, `cells/services/vfs/src/dispatch.rs:292`); `ReadGrant` needs a real VFS handle producer first (`cells/services/vfs/src/handle_table.rs:55`).

## Data Flow

- Copy-out migration after Phase 03 only: caller -> masked/attested VFS request -> `can_read`/seal check -> scoped frame lifetime begins -> VFS mount/overlay resolution -> bounded reply/grant copy -> lifetime closes/acks -> caller-owned buffer exits.
- Target handle read: caller dir handle + name -> VFS validates relative scope -> durable file handle owned by `Caller { cell, generation }` -> each read rechecks owner and policy -> close/revoke/cell death reaches terminal state.
- Revoke/cancel: close/revoke atomically tombstones before freeing state; grant-frame copy needs a scoped lifetime even when synchronous because caller death/preemption can race VFS copy. Rollback is explicit source/operator action, never runtime fallback.

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 01 | [Freeze Decision Contract](./phase-01-freeze-decision-contract.md) | completed | - |
| 02 | [Copy-Out Compatibility Adapter](./phase-02-copy-out-compatibility-adapter.md) | completed | 01, 03 |
| 03 | [Lifecycle Cleanup Checkpoint](./phase-03-lifecycle-cleanup-checkpoint.md) | completed | 01 |
| 04 | [File Handle Bounded Reads](./phase-04-file-handle-bounded-reads.md) | completed | 02, 03 |
| 05 | [Migrate Callers And Retire DataPtr](./phase-05-migrate-callers-and-retire-dataptr.md) | in-progress | 04 |
| 06 | [Closure Verification And Rollback](./phase-06-closure-verification-and-rollback.md) | pending | 05 |

## Checkpoints

- Law 1 checkpoint A: before any `libs/api/` VFS enum or syscall ABI edit; needs two explicit user confirmations (`docs/code-standards.md:12`). Phase 04's exact checkpoint is [recorded here](./reports/phase-04-exact-checkpoint.md): existing 2026-08-09 confirmations cover only the exact append-only `ViVfsFileHandle` / request 23-25 / response 9 delta, with no syscall, manifest, `libs/types`, or fast-IPC expansion.
- Scoped frame-lifetime checkpoint: before Phase 02 edits that migrate any caller to `ReadFileGrant`, approve one scoped VFS grant-copy lifetime design from [the decision package](./reports/phase-03-frame-lifetime-decision-package.md). Existing Law 1 confirmations cover only the already planned append-only handle delta and reserved-slot disablement; they do not cover userspace VFS syscall/semantic authority bridges, operation-scoped pin/lease tokens, new syscall numbers, wire/manifest edits, or lifecycle authority.
- Lifecycle checkpoint: no durable file handles until an audited matrix proves cleanup of kernel caps, VFS handles/pending reads, grants/quarantine, and fast state for Exit, ForceExit, fault, watchdog, hot-swap, service restart, caller death, and cancellation.
- Authority checkpoint: `NotifyOnExit` is SpawnCap-gated (`kernel/src/task/syscall.rs:2281`); do not grant VFS broad SpawnCap just for cleanup. VFS cannot be authorized to "watch held resources" from private service tables the kernel cannot see. Stop unless Phase 03 selects a provable supervisor bridge or a separately approved kernel-visible registry/service-specific death delivery path.
- Law 1 checkpoint B: before disabling `GetFile/DataPtr` serving. Keep enum variants/discriminants reserved; physical removal/renumbering needs a later major-ABI approval.
- Separate approval required for any syscall number, wire format, manifest bit, fast-IPC reachability, or Spec 17 amendment beyond the approved ABI delta.
- Confirmation log: user explicitly supplied Law 1 confirmation #1 and #2 on 2026-08-09. They cover only the append-only handle delta and reserved-slot disablement defined here; any expanded delta requires a new confirmation pair.
- Semantic checkpoint log: user explicitly approved the Phase 03 semantic checkpoint on 2026-08-09, scoped to the per-request VFS grant-copy lease and current-caller-cell-only death watch in `reports/phase-03-recommended-semantic-bridge.md`. This does not authorize a syscall number, wire, manifest, `libs/api`, or `libs/types` change.
- Phase 03 execution evidence: `cargo fmt --all --check`; `cargo test -p types -p api --target x86_64-unknown-linux-gnu`; `bash scripts/build-test-hooks-ci.sh`; RV64 QEMU `vfs_lifetime_selftest_passes` 1/1; RV64 QEMU `riscv64_vfs_quota_all_pass` 1/1; RV64/AArch64/x86_64 production kernel builds; `git diff --check`; standard production review PASS; focused security review PASS. AArch64 test-hooks runtime remains unclaimed because the pre-existing `qemu_exit::AArch64Semihosting` compile issue is still host-gated.
- Phase 04 execution evidence: `cargo test -p types -p api --target x86_64-unknown-linux-gnu` (78 API, 2 contract, 10 types); `cargo check -p service-vfs --target x86_64-unknown-linux-gnu --no-default-features`; `bash scripts/build-test-hooks-ci.sh`; RV64 QEMU `vfs-quota` with 7 table markers plus a valid post-seal handle read and path-addressed denials; RV64/AArch64/x86_64 production kernel builds; `cargo fmt --all --check`; `git diff --check`; standard production review PASS; final domain-risk review CLEAR. Host `cargo test -p service-vfs` stayed unavailable on the `no_std`/unwind host path and was substituted by the QEMU runtime lane; no hardware runtime claim is made. Global coverage debt remains 36.84% line / 35.85% branch, pre-existing.

## Dependencies

Serial execution remains required. Phases 02, 03, and 04 are complete. Phase 05 retirement and Phase 06 remain gated. No phase starts before blockers and stop conditions in its phase file are satisfied.

Exact future ownership is frozen in [file-change-manifest.md](./file-change-manifest.md); completed Phase 03 changes and rollback are recorded in [the execution report](./reports/phase-03-execution.md).

## Scope Stops

No Tier 2/per-domain page tables, async DMA, `RecvScatter`, generic reactor, SMP, identity-less fast IPC, raw-pointer-as-revocable-cap, implicit syscall-authority widening, or broader Midori reopen. Fast IPC may remain only with kernel-attested identity and exact auth parity; otherwise implementation disables its VFS arm rather than restoring old fast `GetFile`.

## Validation Log

Deep red-team clearance is `CLEAR` ([report](./reports/final-red-team-clearance.md)); final fact-check revalidated 4/4 corrections with 0 blockers ([report](./reports/final-validation.md)). Phase 03 evidence is in `reports/phase-03-execution.md`; Phase 02 evidence and caller characterization are in `reports/phase-02-execution.md` and `reports/phase-02-caller-transport-matrix.md`.

Phase 04 exact semantic/ABI checkpoint is in `reports/phase-04-exact-checkpoint.md`; it is design-only and authorizes no product-code edit by itself. The corresponding execution record is `reports/phase-04-execution.md`. Phase 05 migration evidence and the not-ready checkpoint B disposition are in `reports/phase-05-caller-migration-execution.md`.

## Handoff

Plan approved on 2026-08-09; Law 1 confirmations #1 and #2 were recorded the same day. Phase 05 callers now use bounded handle reads and the production raw-pointer inventory is clean outside VFS/tests/ABI fixtures. Do not retire message or fast `GetFile` serving until the missing Lua and QEMU runtime evidence is recorded and checkpoint B is explicitly opened.
