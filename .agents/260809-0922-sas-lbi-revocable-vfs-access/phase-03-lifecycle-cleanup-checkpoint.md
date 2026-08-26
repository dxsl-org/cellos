---
phase: 3
title: "Lifecycle Cleanup Checkpoint"
status: completed
priority: P1
effort: "1d"
dependencies: [1]
tier: thinking
---

# Phase 03: Lifecycle Cleanup Checkpoint

## Overview

Completed gate. The universal cleanup path and scoped VFS grant-copy frame lifetime are approved and verified before Phase 02 copy-out migration or durable VFS file handles ship. This phase remains a checkpoint: if cleanup needs forbidden authority expansion, stop.

## Requirements

- Functional: characterize Exit, ForceExit, fault, watchdog, hot-swap, service death, caller death during VFS copy, VFS provider death/restart, and cancellation behavior for every durable VFS object and scoped grant-copy lease.
- Non-functional: no public ABI, syscall number, manifest bit, or VFS SpawnCap grant in this phase.
- Decision: select one scoped frame-lifetime option from `reports/phase-03-frame-lifetime-decision-package.md` before Phase 02 resumes.

## Architecture

Cleanup remains distributed but now has an audited terminal contract. Scheduler death queues grant/VFS-lease cleanup for draining outside `SCHEDULER`; Exit/ForceExit and hot-swap use the same quarantine/release primitives, and hot-swap acknowledges ordinary DMA pins only after IOMMU cleanup. Calling `exit_task` alone is still not cleanup proof.

VFS state now records each watched owning task against an exact `Caller { cell, generation }`. A kernel-attributed owner death purges matching directory handles, file handles, and pending reads immediately; higher-generation lazy purge remains defense-in-depth, not the cleanup proof.

`ReadFileGrant` still copies through `sys_grant_slice_with_len`, but registered VFS `GrantSlice` now enters an exact operation-scoped lease before returning the address. Matching `Send` releases by holder, owner, and request generation; owner death quarantines before frame reuse, while ordinary DMA acknowledgement remains owner-wide and cannot release VFS leases.

The terminal contract spans the scheduler death funnel, deferred cleanup drains, grant reaper, pin registry, fast-state clearing, and VFS owner-death purge. Frame release occurs outside `SCHEDULER`; VFS never holds its state lock across `NotifyOnExit`.

The approved `NotifyOnExit` exception is service-specific: only the kernel-registered VFS task may watch the kernel-derived owning task of its current caller. Authorization and subscription publish atomically under `SCHEDULER -> DEATH_SUBSCRIBERS`; VFS gains neither broad `SpawnCap` nor authority derived from private handle tables.

## Resolved Assumptions

- The kernel-visible current-caller rule supplies immediate owner death events without broad VFS `SpawnCap`; QEMU covers permitted owner watch, arbitrary denial, worker/owner separation, and already-dead delivery.
- The existing pin registry was extended with a bounded VFS lease table and exact release key; it does not reuse owner-wide DMA acknowledgement.

## Related Files

- Modified: kernel pin/task/scheduler/syscall/hot-swap/fast-VFS registration paths.
- Modified: VFS owner-watch and exact-generation dir/file/pending cleanup paths.
- Added: test-hook boot self-test and RV64 QEMU integration assertion.
- Unchanged: `libs/api`, `libs/types`, syscall numbers, wire formats, and manifests.

## Implementation Steps

1. Build a matrix for Exit, ForceExit, fault, watchdog, heartbeat, hot-swap, caller death during VFS copy, VFS death/restart, and cancellation against kernel caps, VFS file/dir handles, pending reads, grants/pins/quarantine, fast state, reply/error, and restart re-registration.
2. Decide lifecycle bridge:
   - Option A: one audited kernel terminal helper plus a supervisor bridge; accept only with provable full coverage, restart re-registration, and no broad VFS SpawnCap.
   - Option B: supervisor bridge; accept only if coverage of every cell and restart is proven.
   - Option C: explicit kernel-visible registry/service-specific death delivery; requires separate approval and must avoid general supervisor authority.
   - Option D: generation purge plus bounded sweep; containment only, rejected as the durable-handle gate.
3. Decide scoped VFS frame lifetime:
   - Option A: adapt existing pin registry for service-side grant copy only with operation-scoped generation/token and exact release; separate semantic checkpoint required unless it is wholly existing kernel-mediated behavior with no new authority semantics.
   - Option B: add scoped lease/token/ack; needs a new checkpoint if it changes `libs/api`, syscall numbers, wire, manifest, or authority bits.
   - Option C: kernel-mediated copy; feasible only if it reuses existing syscalls/wire or gets a new checkpoint for any copy syscall/API surface.
4. Prove stop rule: no Phase 02 copy-out migration and no durable handles until every terminal row performs immediate owner cleanup or a separately approved equivalent. Fail-closed successor denial does not satisfy cleanup by itself.
5. Document lock order: VFS locks must not be held across kernel syscalls; grant teardown order remains pin/table -> frame allocator -> root page table (`kernel/src/memory/pin.rs:40`, `kernel/src/task/syscall.rs:242`).
6. Request only the minimal checkpoint: approve one scoped-copy lifetime plus one VFS-provider death subscription/cleanup helper design. Request a new Law 1 pair only if the selected option touches `libs/api/` or `libs/types/`.

## Success Criteria

- [x] Cleanup matrix lists Exit, ForceExit, fault, watchdog, hot-swap, service death, and cancellation.
- [x] Scoped VFS frame-lifetime option is selected with evidence and explicit checkpoint boundary.
- [x] Terminal cleanup helper contract covers grants/pins/quarantine, VFS handles, pending reads, fast state, and subscriptions.
- [x] VFS provider death subscription uses either a provable supervisor bridge or separately approved kernel-visible registry/service-specific death delivery; VFS-private ownership watch is rejected.
- [x] Each terminal path has immediate cleanup proof; containment-only rows keep Phase 04 disabled.
- [x] No VFS SpawnCap expansion is introduced.
- [x] User explicitly gave Law 1 confirmation #1 and #2 on 2026-08-09; Phase 04 must stay within the approved append-only delta.

## Security Considerations

Durable handles without universal death cleanup can become successor authority. Unknown/wrong-owner/stale-generation results must remain indistinguishable to avoid handle probing.

## Risk Notes

- Risk High x Critical: direct VFS notification could grant broad SpawnCap. Mitigation: kernel-derived current-caller owner rule only; arbitrary watch denies fail closed.
- Risk High x Critical: VFS copy could write into freed/reused grant frame. Mitigation: exact lease, owner-death quarantine, and exact completion/holder-death release.
- Risk Medium x High: lazy purge hides leaks. Mitigation: distinguish containment from immediate cleanup in success criteria and tests.
- Rollback: revert the Phase 03 kernel/VFS/test slice together; never retain lease creation without matching completion and death cleanup. No ABI or manifest rollback is required.
- Stop condition for follow-on work: any new path needs broad SpawnCap, identity-less fast IPC, Tier 2, async DMA, or an unapproved syscall/wire/manifest change.

## Deviation Log

- 2026-08-09: Promoted Phase 03 to next gate and made Phase 02 dependent on it after discovering synchronous `ReadFileGrant` has no scoped frame lifetime across caller death/preemption.
- 2026-08-09: User approved the narrow semantic bridge in `reports/phase-03-recommended-semantic-bridge.md`: exact per-request VFS grant-copy leases plus current-caller-cell-only death watch, with no syscall number, wire, manifest, `libs/api`, or `libs/types` change.

## Validation

- Approved bridge: exact per-request VFS grant-copy lease plus current-caller-cell-only death watch, with no `libs/api`, `libs/types`, syscall number, wire, or manifest change.
- Verification: `cargo fmt --all --check`; `cargo test -p types -p api --target x86_64-unknown-linux-gnu`; `bash scripts/build-test-hooks-ci.sh`; RV64 QEMU `vfs_lifetime_selftest_passes` 1/1; RV64 QEMU `riscv64_vfs_quota_all_pass` 1/1; RV64/AArch64/x86_64 production kernel builds; `git diff --check`.
- Reviews: standard production review PASS; focused security review PASS.
- Caveat: AArch64 test-hooks runtime is still not claimed because the pre-existing `qemu_exit::AArch64Semihosting` compile issue remains host-gated.
