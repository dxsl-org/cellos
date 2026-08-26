---
phase: 2
title: "Cut Over Atomic Commit and Every Caller"
status: completed
priority: P1
effort: 1d
dependencies: [1]
tier: thinking
---

# Phase 02: Cut Over Atomic Commit and Every Caller

> **Required — deviation-log:** Record each Decision / Deviation / Surprise immediately. Choose the smallest reversible response; escalate any contract-breaking change.

## Overview

Move governed and trusted launch configuration ahead of ready publication, migrate every caller in one clean cutover, and remove post-spawn security mutation/denial.

## Requirements

- Governed preflight retains the current manifest classification, signature gate, privilege rule, spawner ceiling, operator policy, allowlist, cluster, quota, capability, protection-class, singleton, measurement, and route behavior.
- All denials that can be decided from bytes/path/caller/route run before ELF resource allocation; reservations acquired later remain rollback-owned.
- Trusted embedded init bypasses governed signature/manifest admission exactly as today, but receives boot ceiling, `SupervisorCap`, `is_critical`, identity, and context before ready.
- `SpawnPinned` validates priority and cluster together before publication and writes priority before ready; it never spawns then calls `exit_task` for denial.
- `SpawnReplacement` reserves its one-shot ceiling and source binding, restores both on failure, and binds the new task before ready.
- Directory handles and argv are transferred before ready. The syscall layer may retain its documented consume-on-attempt policy, but the core transaction snapshot must be exact before that outer consumption occurs.

## Exact Data Flow

1. Entry point validates route inputs and snapshots caller launch authority/generation.
2. Loader creates immutable `SpawnDecision`: parsed manifest version/value, allowlist, cluster, requested/granted caps, PKU values, quota, priority, route flags, staged digest/path, and reservations.
3. Loader revalidates caller authority/generation at publication when the request came from a live task; a concurrent cap revoke cannot authorize a child from a stale ceiling.
4. `prepare_elf_task` builds all owned memory/context.
5. `publish_prepared` derives TID/`CellId`, validates quota slot, installs fields/inheritance/argv/replacement binding, commits reservations and success evidence/routes, inserts the task, then pushes ready last while `SCHEDULER` remains locked.
6. Only after success returns may the syscall report the TID. No caller performs required task mutation or denial cleanup.

## Caller Inventory and Migration

| Caller | Required migration |
|---|---|
| `loader::spawn_from_path` | Accept/pass `SpawnRequest`; keep path validation and early-file lookup. |
| `loader::spawn_gated` | Become preflight + prepare + publish coordinator; no post-spawn TCB writes or denials. |
| `loader::mem_spawn_gate::spawn_from_mem_gated` | Pass neutral `/mem/` label and caller request; preserve no path-derived authority. |
| `Syscall::SpawnFromPath` | Supply caller identity/generation, dirs, argv, default priority. |
| `Syscall::SpawnFromElf` | Same as path while retaining grant-owned byte validation. |
| `Syscall::SpawnFromMem` | Same via neutral label; public ABI unchanged. |
| `Syscall::SpawnPinned` | Put priority/core constraint into request; move RT+cluster denial before publish. |
| `Syscall::SpawnReplacement` | Pass replacement reservation; delete spawn-then-bind/kill branch. |
| `main` Platform spawn | Continue governed `Spawner::Root` path. |
| `main` embedded init | Replace raw spawn + direct TCB mutation with explicit trusted-complete request. |
| Loader/tests/comments | Rename references to removed raw API and assert ready means complete. |

## No-Shim Rule

Remove `task::spawn_from_mem` after migrating its two direct call sites (`loader.rs`, `main.rs`). Do not retain a wrapper, alias, deprecated export, overloaded default, or public builder with optional security fields. Keep public userspace spawn syscall layouts unchanged; this is an internal kernel cutover, not an ABI migration.

## Related Files / Ownership

- Modify: `kernel/src/loader.rs` — governed preflight, decision construction, no post-spawn mutations.
- Modify: `kernel/src/loader/mem_spawn_gate.rs` — request propagation and comments.
- Modify: `kernel/src/task/syscall.rs` — all five syscall routes, priority, argv/dirs, replacement.
- Modify: `kernel/src/main.rs` — governed Platform call and trusted-complete init call.
- Modify: `kernel/src/task/dir_inherit.rs` and `kernel/src/cell/state_stash.rs` only if an owned pre-publication transfer API is required.
- Modify: `kernel/src/cell/hotswap.rs` only for the caller-facing reservation handoff defined in Phase 01.

## Implementation Steps

1. Parse allowlist/cluster and compute caps/policy/protection/route denials before `prepare_elf_task`; keep signature code unchanged.
2. Replace loader post-spawn sections with `TaskLaunchState` construction and one publication call.
3. Fold success-only block-I/O/VFS handler and input endpoint registration into the infallible pre-ready commit; capture/restore prior route values for injected precommit failures.
4. Migrate SpawnPinned and SpawnReplacement denial/binding logic into request/precommit state.
5. Move argv and directory inheritance into publication, preserving the outer syscall's intentional failure-consumption semantics.
6. Migrate init with an explicit trusted constructor; remove its scheduler lookup/direct writes.
7. Remove old raw API and update every comment/caller in the same change.

## Success Criteria

- [ ] No governed or trusted caller can observe/return a TID before the child is fully configured and ready.
- [ ] No task required field (`cell_id`, context, segment owner, allowlist, cluster, caps, PKU, priority, init critical/supervisor state, inheritance/replacement) is mutated after ready publication.
- [ ] SpawnPinned denial and replacement bind failure create no task/zombie/measurement/quota residue.
- [ ] Direct init retains its exact authority and bypass behavior without a runnable-default window.
- [ ] Raw `task::spawn_from_mem` has zero definitions/references and no compatibility path exists.

## Security Considerations

The caller ceiling must be revalidated against the same task generation at commit to prevent revoke/spawn races. Do not expand `/bin/` trust, `Spawner::Root`, mem-label authority, or signed byte coverage.

## Risk Notes

Success measurement and success audit are append-only; they must occur after the final possible denial/injection and before ready. They cannot be used as rollback-managed staging after publication.

## Assumptions

- **Claim:** Operator policy is resolved once at boot and not hot-reloaded during spawn.
  **Confidence:** high
  **How to verify:** Re-read all writes to `policy::POLICY` before Build; if reload exists, include policy generation in revalidation without expanding scope.

## Deviation Log

None.
