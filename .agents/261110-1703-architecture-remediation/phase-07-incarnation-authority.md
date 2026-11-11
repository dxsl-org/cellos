---
phase: 7
title: "One authority record per Cell incarnation"
status: in-progress
priority: P1
effort: "8–12d"
dependencies: [6]
tier: thinking
checkpoint: true
---

# Phase 07: One authority record per Cell incarnation

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-1, Medium authority](../reports/architecture-review-261110.md) · `kernel/src/task/cap.rs` · `kernel/src/task/tcb.rs` · `kernel/src/cell/cspace.rs` · `kernel/src/task/syscall.rs` · `kernel/src/audit.rs` · [Spec 25](../../docs/specs/25-cell-kernel-authority-contract.md) · owner decision 2 (Phase 00)

## Overview

Make "what a Cell holds" one kernel record keyed by `(cell_id, generation)`, read by every gate and written by revoke, so threads cannot retain revoked authority and CSpace stops being a write-only second truth.

## Key Insights

- Reviewer-cited: gates read the calling thread's TCB (`syscall.rs:1977-1988`); threads get a copy (`scheduler.rs:1171`); `CapRevoke` clears one tid (`syscall.rs:7101-7128`); `CellAuthority.ceiling/current_rights` are written and never read (`cspace.rs:404-405`, `syscall.rs:7152`).
- `scheduler.rs:613,685` already registers and retires a CSpace row per incarnation — the record exists; it is not consulted.
- This is a kernel-internal change: no syscall number or trailer layout moves.

## Requirements

- Functional: `caller_has_cap` and service-flag attestation resolve through the incarnation record; revoke affects every thread of the incarnation atomically; kernel caps and service flags follow one rule; audit records carry `(cell, generation, tid)`; the set of service-right bits carried in `CapSet` is frozen (documented, no new ones).
- Non-functional: gate cost stays O(1) without taking `SCHEDULER`; no allocation on the syscall path.

## Architecture

- Default (decision 2): `CellAuthority` in `cell/cspace.rs` becomes the authoritative record; each TCB holds `Arc<CellAuthority>` (or an index + generation into the existing arena); rights are atomics or behind the record's own leaf lock.
- Delete `CapSet::apply_to`, `of_task`, `from_task` and the per-TCB cap fields (`tcb.rs:357-422`); non-cap token objects that must stay per-task (e.g. the platform singleton reservation) move into the record with RAII release at incarnation retirement.
- Alternative (decision 2 = cut): same record as a new small type; `cspace.rs` reduced to the BootFS file-ticket table.

## Assumptions

- **Claim:** every per-TCB capability field has cell-wide meaning (none is intentionally per-thread).
  **Confidence:** medium
  **How to verify:** read `tcb.rs:357-422` and `thread_cap_selftest`; list any field narrowed per thread; those become explicit sub-limits, not copies.
- **Claim:** the incarnation row outlives every thread of the Cell.
  **Confidence:** medium
  **How to verify:** read the retire path at `scheduler.rs:685` against thread exit ordering.
- **Claim:** hot-swap ceilings (`cell/hotswap.rs:30`, tid-keyed) can key on incarnation without changing swap semantics.
  **Confidence:** low
  **How to verify:** read `hotswap.rs:113-135`.

## Related Code Files

- Modify: `kernel/src/cell/cspace.rs`, `kernel/src/cell/cap_registry.rs`, `kernel/src/task/{cap,tcb,scheduler,syscall}.rs`, `kernel/src/task/launch.rs`, `kernel/src/cell/hotswap.rs`, `kernel/src/resource_registry.rs`, `kernel/src/cell/service_registry.rs`, `kernel/src/audit.rs`, `kernel/src/loader/governed_spawn.rs`, `kernel/src/task/thread_cap_selftest.rs`, `scripts/mutable-globals.toml`
- Delete: `kernel/src/cell/registry.rs` (unused `CELL_REGISTRY`), duplicate `CapSet` constructors

## Implementation Steps

1. Inventory every reader and writer of per-TCB capability state (LSP references on each field); classify as gate, attestation, spawn, revoke, teardown.
2. Add the record handle to the TCB; populate at publication; keep old fields in parallel with a debug assert that both agree.
3. Switch gates, then attestation, then revoke, one commit each, with the agreement assert on.
4. Rewrite `thread_cap_selftest` to assert: revoke on any thread removes the cap from all siblings; a new thread never widens.
5. Remove TCB fields and copy functions; key MMIO ownership, service owners and swap ceilings on incarnation where they are tid-keyed.
6. Audit: one subject tuple in every record; admission denials logged against the caller.
7. Update the writable-global inventory.

## Todo List

- [ ] Reader/writer inventory
- [ ] Parallel record with agreement assert
- [ ] Gates, attestation, revoke switched
- [ ] Selftest rewritten
- [ ] TCB fields removed
- [ ] Audit subject tuple

## Success Criteria

- [ ] Selftest: after `CapRevoke` targeting a worker tid, the root and all siblings fail the gate (fails on the tree before this phase — record the before-result).
- [ ] Kernel host tests and x86/RV64/AArch64 boot lanes pass.
- [ ] No non-test reader of a per-TCB cap field remains (grep/LSP zero).
- [ ] Checkpoint: full `pr`-tier suite, `x86-c2c`, 2-CPU x86 domain lane, independent review of the diff recorded.

## Risk Assessment

- Lock ordering: the record's lock must be a leaf below `SCHEDULER`; document rank now (enforced in Phase 10).
- Semantics change is intended (revoke becomes Cell-wide); any consumer relying on per-thread retention is a bug to list, not preserve. Rollback: source-only revert to TCB fields.

## Security Considerations

Closes the stale-authority window Spec 15 §1.2 says only the kernel can close. Revocation remains an admission cut; in-flight effects are not rolled back.

## Next Steps

Phase 08 may now move authority code out of `syscall.rs` into `task/authority/`.

## Deviation Log

- **Before-result (step 1/4):** new host test `cap_revoke_on_a_worker_removes_the_right_from_every_thread_of_the_cell` on the unmodified tree: `tid 202 kept SpawnCap`. The root kept the right a revoke aimed at its worker thread removed. The revoke core was extracted into `commit_cap_revoke` so the test can run on the host; the full arm notifies the victim through a mailbox the host cannot provide.
- **Decision (record shape, D2 deviation):** the incarnation record is the Cell root's `Task::rights: CapSet`, read through `Scheduler::cell_rights(tid)` (task → `root_tid` → record). It is not an `Arc<CellAuthority>`. Reasons:
  - It needs no allocation. `Arc::new` is infallible, and spawn and thread paths must not panic on OOM.
  - The gates already hold `SCHEDULER`, so the lookup is O(1) under it.
  - The root lives exactly as long as the incarnation, and a retiring root fails closed with `EMPTY`.
  - `CellAuthority.ceiling/current_rights` (written, never read) are removed in step 5, so CSpace stops being a second truth.
  - NFR "gate without taking SCHEDULER" is not met: gates take it today as before. Lock ranks belong to Phase 10.
- **Deviation (steps 2–4 merged):** a "parallel record + agreement assert" commit has nothing to compare once the record is the root's own field, and switching the gates without the selftest leaves every boot selftest red. So the gates, attestation, revoke, thread spawn (no copy) and the selftest rewrite land in one commit; field removal and audit follow.
- **Behaviour change:** `CapRevoke` on a thread of the caller's own incarnation is refused (`InvalidCommand`), as `target_tid == caller_id` already was. With Cell-wide rights it would revoke the caller itself. Teardown (MMIO, DMA, grants) now runs for every tid of the target incarnation; resources stay tid-keyed, and the members are collected into a fixed 33-entry buffer with no allocation.
- **Surprise (C-1 leftover):** `CellSessionV1` launch re-validation (`launch.rs` `PreparedSessionLaunch::current`) still authorised with `caller.name`, the task name. It now uses `syscall::launch_identity_in`, the same measured identity as `caller_launch_state`.
- **File set (to confirm at close):** two gate readers outside request §2 had to switch: `kernel/src/task.rs` (`paused_target_rejects`, a SupervisorCap gate) and `kernel/src/task/cell-image.rs` (441 SupervisorCap gate). Every other change is inside §2 or is a kernel selftest that builds tasks with caps.
- **Evidence (gate-switch commit):** 611 host tests pass, including the before-failing revoke test. Boot selftests on x86, RV64 and AArch64 all log `thread-cap self-test PASS`, which now includes the new `REVOKE-CELL-WIDE` check. Its first run failed because the parent held `network`, which marks a system Cell that `CapRevoke` refuses; the fixture now uses `supervisor`.
