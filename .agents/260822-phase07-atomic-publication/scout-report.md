# Phase 07 Atomic Publication Scout Report

## Confirmed Lifecycle

- `kernel/src/task.rs:1047-1288`: `spawn_from_mem` allocates PIE VA/segments/stacks, then calls `spawn_cell_task`; context and segment ownership are installed only afterward.
- `kernel/src/task.rs:987-1009` and `kernel/src/task/scheduler.rs:237-330`: `spawn_cell_task` reaches `spawn_with_stacks_configured`, which inserts the task and pushes it to a per-hart ready queue before returning. Only identity and directory inheritance are protected by that scheduler lock.
- `kernel/src/loader.rs:197-391`: governed loading calls the already-published task API, then writes success audit/measurement, allowlist, cluster, quota, granted caps, PKU state, Platform singleton state, VFS handler ownership, and input endpoint state. VFS block-region and Platform singleton denials occur after task publication.
- `kernel/src/main.rs:884-918`: trusted embedded init calls raw `task::spawn_from_mem`, then writes boot ceiling, `SupervisorCap`, and `is_critical` after the task is ready.
- `kernel/src/task/syscall.rs:2924-3004`: SpawnPinned transfers argv, checks RT+cluster, may exit an already-ready denied task, and only then writes priority.
- `kernel/src/task/syscall.rs:3886-3958`: SpawnReplacement consumes a frozen ceiling, publishes a child, then binds it; bind failure uses ordinary exit cleanup rather than exact transaction rollback.
- `kernel/src/task/syscall.rs:539-546`: argv transfer occurs after publication. Directory inheritance is installed inside current task registration, while outer syscall routes clear staged state on every attempt.

## Confirmed Cleanup Surfaces

- `kernel/src/task/stack.rs:345-420`: `Stack` and `CellSegments` own frame/mapping/PIE cleanup through `Drop`; these are the base RAII mechanism.
- `kernel/src/task/scheduler.rs:471-625`: `exit_task` moves a task to zombies and changes multiple global queues/notifications; it cannot provide exact never-published rollback and does not restore `next_task_id`.
- `kernel/src/memory/cell_quota.rs:59-80`: quota registration/deregistration is separate global state; IDs beyond `MAX_CELLS` are not fully enforced by the atomic usage array.
- `kernel/src/task/cap.rs:107-128`: Platform singleton is a permanent atomic flip with no uncommitted reservation owner.
- `kernel/src/cell/hotswap.rs:57-102`: replacement ceiling is consumed before spawn and binding is a separate post-spawn operation.
- `kernel/src/measurement_log.rs:50-86`: measurement and aggregate are append-only and therefore must be committed only after the last possible denial.

## Existing Proof Gap

`kernel/src/loader/manifest_section_tests.rs:124-238` snapshots tasks, zombies, next TID, hart attribution, and ready queues only for malformed-manifest denials that occur before allocation. It does not cover post-allocation failures, quota, singleton, mappings/frames/VA slots/stacks, routes, measurement aggregate, SpawnPinned, SpawnReplacement, or first-runnable trusted init state.

## Scope Boundaries

The plan deliberately leaves `CELLOS-LOADER-SIG-001`, signed-byte coverage, provenance, AddressSpace/native domains, native-domain admission, Manifest v3 layout/parser/writer, execution-tier metadata, and UI untouched.
