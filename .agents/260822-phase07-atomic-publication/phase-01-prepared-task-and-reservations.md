---
phase: 1
title: "Prepare Owned Task and Rollback Primitives"
status: completed
priority: P1
effort: 1d
dependencies: []
tier: thinking
---

# Phase 01: Prepare Owned Task and Rollback Primitives

> **Required — deviation-log:** Record each Decision / Deviation / Surprise immediately. Choose the smallest reversible response; escalate any contract-breaking change.

## Overview

Separate fallible ELF/resource construction from scheduler publication and give every pre-publication claim one explicit owner whose `Drop` restores it.

## Requirements

- `PreparedElfTask` exclusively owns segment mappings/frames, PIE VA slot, kernel/user stacks, entry/context inputs, and load base until publication.
- `TaskLaunchState` is complete plain data: identity request, allowed drivers, syscall allowlist, cluster, granted capabilities, protection values, priority, boot-only supervisor/critical fields, inheritance payload, argv payload, route side effects, measurement digest/path, and optional replacement source.
- `PlatformCapReservation` resets the singleton latch on denial/drop and becomes non-releasable only when moved into the published task; successful one-holder-ever behavior remains unchanged.
- Quota registration has an owned reservation/lease with exactly-once deregistration before commit. A derived `CellId` outside the enforceable quota range is denied, never silently uncapped.
- Replacement ceiling consumption is an RAII reservation: failed spawn restores the exact source ceiling; successful commit consumes it and binds the source before ready.
- No destructor may run while holding `SCHEDULER` if it takes `FRAME_ALLOCATOR`/page-table locks; failed commit returns owned resources for drop after unlocking.

## Architecture / API Contract

- `task::prepare_elf_task(data, name, requested_cell_id, allowed_drivers) -> Result<PreparedElfTask, ViError>` performs parse, VA/segment allocation, relocation, W^X, stack allocation, and context preparation without touching scheduler/task IDs/ready queues.
- `task::publish_prepared(prepared, TaskLaunchState) -> Result<(tid, load_base), ViError>` is the only path from a prepared ELF to runnable state.
- `cap::reserve_platform() -> Result<PlatformCapReservation, ViError>` and `PlatformCapReservation::commit_into(&mut Task)` encode rollback vs permanent grant; remove direct loader use of `try_grant_platform`.
- Quota and hot-swap modules expose crate-private reservation types, not boolean acquire/release pairs. Commit consumes each token; `Drop` is rollback.
- Publication accepts complete data, not a fallible configuration closure. `spawn_with_stacks_configured` may remain only for non-ELF synthetic tests; it is not a compatibility path for cell loading.

## Invariants

1. Before `publish_prepared`, the task ID does not exist in `Scheduler::tasks`, zombies, any per-hart ready queue, registries, quota tables, or measurement-success log.
2. Dropping any prepared object restores every owned frame, mapping/flags, stack guard mapping, and PIE slot exactly once.
3. A failed reservation leaves singleton/quota/replacement stores byte-for-byte equivalent at their public/test snapshot surface.
4. The commit consumes every owner exactly once; no manual cleanup duplicates RAII cleanup.

## Related Files / Ownership

- Modify: `kernel/src/task.rs` — prepared task and sole ELF publication API.
- Modify: `kernel/src/task/scheduler.rs` — final configured insert/ready-last primitive and rollback return path.
- Modify: `kernel/src/task/stack.rs` — expose only the ownership/snapshot hooks required by preparation tests.
- Modify: `kernel/src/task/cap.rs` — singleton reservation.
- Modify: `kernel/src/memory/cell_quota.rs` — quota lease, range denial, test snapshot.
- Modify: `kernel/src/cell/hotswap.rs` — replacement-ceiling reservation and pre-publication bind primitive.

## Implementation Steps

1. Extract all work through current `task::spawn_from_mem` step 7 into `PreparedElfTask`; retain `CellSegments`/`Stack` Drop ordering and remove scheduler registration from preparation.
2. Add complete launch-state types with private fields and governed/trusted constructors; constructors must require every security-sensitive field explicitly.
3. Replace the permanent atomic singleton flip with reservation ownership; prove only the owning uncommitted token can reset it.
4. Add quota and replacement reservations with test-only snapshots; preserve lock order and ensure failure drops happen after scheduler unlock.
5. Implement scheduler commit with one last failure checkpoint before mutation, full TCB configuration, task-table insert, task-id advance, route/measurement success commit, and `push_ready` as the final operation.
6. Delete any API that can insert an ELF task with defaults or mutate required launch state after ready.

## Success Criteria

- [ ] Every pre-publication resource has one named RAII owner and one commit transfer.
- [ ] There is exactly one ELF task publication API and no fallible callback after commit starts.
- [ ] Derived identities outside quota enforcement are denied without publication.
- [ ] Platform and replacement reservations roll back on all uncommitted drops.

## Security Considerations

Do not touch signature parsing, signed byte coverage, provenance, or manifest format. A reservation reset must be owner-checked so one failing spawn cannot clear another spawn's committed singleton.

## Risk Notes

`BTreeMap` insertion/allocation and measurement-log allocation must not introduce a recoverable error after irreversible mutation. Stage allocations before commit or structure the commit so the only remaining operations are infallible under the kernel allocator contract.

## Assumptions

None — lifecycle, cleanup, quota, singleton, and replacement behavior were read directly from the cited kernel files.

## Deviation Log

None.
