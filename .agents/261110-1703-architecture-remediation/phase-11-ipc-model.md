---
phase: 11
title: "One IPC operation model"
status: in-progress
priority: P1
effort: "10–14d"
dependencies: [10]
tier: thinking
checkpoint: true
---

# Phase 11: One IPC operation model

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-6, H-14 (local≠remote), Medium IPC/kernel shape](../reports/architecture-review-261110.md) · `libs/api/src/abi/syscall.rs` · `kernel/src/task/tcb.rs` · `kernel/src/task/async_ipc.rs` · `kernel/src/fast_ipc.rs` · `kernel/src/cell/service_registry.rs` · `libs/ostd/src/{ipc,service,cluster_endpoint,fast_ipc,ring_channel}.rs` · [Spec 17](../../docs/specs/17-ipc-wire-contract.md) · [ADR-0023](../../docs/decisions/0023-local-service-generation-binding.md)

## Overview

Make exact-operation IPC with `WaitCompletion` the one request/reply model, bound the legacy blocking send, remove dead mechanisms, and give the SDK one call surface with typed outcomes so a remote adapter can later slot in.

## Key Insights

- Reviewer-cited: ~12 mechanisms; 11 parked `TaskState` variants (`tcb.rs:40-137`); `Sending` has no deadline (`tcb.rs:47-53`); 67 `sys_send` sites in services/drivers; kernel `Rename` loops on VFS with no expiry (`syscall.rs:7573-7596`); dead arms `Lend/BorrowRead/BorrowWrite/Grant/Map` (`syscall.rs:6907-6915,7445-7466`); `fast_ipc` is self-described scaffolding yet gates generic paths at nine sites; `RemoteEndpoint::call` returns `NotSupported` and local outcomes collapse to `ViError::IO` (`cluster_endpoint.rs:133-141,199-206`); 134 tid-only lookups vs 57 bound.
- Frozen opcodes stay reserved when retired; they are never reused.

## Requirements

- Functional: (a) blocking `Send` gains a mandatory bound in the SDK default (deadline or Busy), with an explicit allowlisted event-only path; (b) service replies use `IpcReply`; (c) kernel→VFS `Rename` uses an exact operation with expiry or moves to userspace; (d) dead arms and `Task` fields removed, opcodes marked reserved; (e) `IpcWait`/`WaitForEvent`/`WaitIrq` become completion sources behind compatibility wrappers in `ostd`; (f) parked states collapse to `Blocked { wait, deadline }`; (g) `ServiceRef::call` delegates to `CellMethod`/`LocalEndpoint` returning `Reply | Refused | Uncertain`; (h) `service::lookup` returns a bound `ServiceBinding`; tid-only lookup and the legacy name lookup are removed after migrating callers; (i) `fast_ipc` deleted on both sides, VFS-specific kernel rules become per-registration flags.
- Non-functional: Law-1 CP1/CP2 for every `libs/api` delta; Spec 17 amended in the same slice.

## Architecture

```
caller ─ CellMethod::call ─► LocalEndpoint ─ IpcSubmit ─► provider ─ IpcReply ─► terminal
                │                                                   └ loss/timeout/cancel
                └► typed outcome: Reply(bytes) | Refused(reason) | Uncertain(reason)
events (input, IRQ, notifications): bounded post, never a reply channel
```

`service_registry` rows gain `retains_outer_caller` and `owner_watch`, set by the namespace owner at registration, replacing `is_registered_vfs_cell` checks and the hard-coded `OWNER_WATCH_SERVICES`.

## Assumptions

- **Claim:** every blocking `sys_send` in a service is a reply and can move to `IpcReply`.
  **Confidence:** low
  **How to verify:** classify the 67 sites (reply / event / request) before designing the bound.
- **Claim:** removing tid-only lookup has no out-of-tree consumer.
  **Confidence:** medium
  **How to verify:** only in-tree consumers can be checked; state this in CP1.
- **Claim:** collapsing parked states does not change snapshot or `GetProcs` layouts visible to userspace.
  **Confidence:** low
  **How to verify:** grep `TaskState` in `libs/api` and `cells/`.

## Related Code Files

- Modify: `libs/api/src/abi/syscall.rs`, `libs/api/src/services/ipc.rs`, `libs/ostd/src/{ipc,service,cluster_endpoint,syscall}.rs`, `libs/ostd/src/clients/**`, kernel IPC files (`async_ipc`, `completion*`, `tcb`, `scheduler`, syscall `ipc.rs`), `kernel/src/cell/service_registry.rs`, every caller of `sys_send`/tid lookup under `cells/services` and `cells/drivers`, `docs/specs/17-ipc-wire-contract.md`
- Create: `cp1-ipc-model.md`, `cp2-ipc-model.md`
- Delete: `kernel/src/fast_ipc.rs`, `libs/ostd/src/fast_ipc.rs`, `cells/services/vfs/src/fast_handler.rs`, dead `Task` fields, `spawn_from_file` stub, dummy waker / `TaskState::Polling`

## Implementation Steps

1. Classify all send/lookup call sites; write CP1 (deltas, reserved opcodes, migration census, negative cases). Owner confirmation 1.
2. Typed outcome + `CellMethod` as the SDK surface; migrate `ostd` clients first, then services, then apps.
3. Bound the SDK send default; convert replies to `IpcReply`; fix the kernel `Rename` path.
4. Registration flags; delete `fast_ipc` and the VFS special cases.
5. Fold the three wait syscalls into completion sources; collapse parked states.
6. Remove dead arms/fields; mark opcodes reserved in the syscall table.
7. Bound lookup everywhere; delete tid-only and name lookups.
8. Amend Spec 17 §1/§6; CP2 with evidence.

## Todo List

- [x] Call-site census, CP1 *(CP1 drafted; owner confirmation pending)*
- [ ] Typed outcome surface
- [ ] Bounded send, `IpcReply` replies
- [ ] Registration flags, `fast_ipc` removed
- [ ] Waits folded, states collapsed
- [ ] Dead mechanisms removed
- [ ] Bound lookup only
- [ ] CP2

## Success Criteria

- [ ] New x86 fixture: a client that never receives does not park the supervisor, config or compositor (each service continues serving a second client) — fails on the tree before this phase.
- [ ] Killing VFS mid-`Rename` returns an error to the caller within the deadline instead of wedging the task.
- [ ] Existing C2C evidence lanes (saturation/deadline/caller-death, restart, peer pressure) pass unchanged.
- [ ] Checkpoint: full `pr`-tier suite on three arches, `x86-c2c`, two-node nightly lane; frozen-surface digest scripts pass or are re-pinned under CP2; review recorded.

## Risk Assessment

- Largest behaviour change in the plan; staged so each step is shippable. Irreversible part: once the outcome enum and reserved opcodes are public they are compatibility surface.
- Rollback: SDK default can be reverted independently; kernel removals revert per commit.

## Security Considerations

Removes the "one non-receiving client parks a Tier-1 service" class (audit 10 S-2, 11 S-6) structurally. Bound lookups close stale-tid confusion after restart.

## Next Steps

Phase 12 builds the shared service loop on this surface.

## Deviation Log

- **Census and CP1** (2026-11-11). The census was run by 3 read-only `Explore` agents and is versioned in `research/p11-census-*.md`. It changed the plan in three ways (CP1 §1):
  - **The hazard is narrower.** The 155 service reply sites already complete exact operations through the kernel's implicit bridge. The non-receiving-client hazard is limited to replies to raw-`Send` callers and to events a service sends while serving.
  - **Possible live misdelivery:** compositor `window.rs:151` may turn its state event into the CONFIGURE_ACK reply.
  - **Requirement (g) moves to `ServiceRef::call`.** Its suggested target, `LocalEndpoint::call`, has no production callers.
- `ProcessInfo` keeps its encoding (no Law-1 change for requirement f).
- No new opcode or allowlist bit: the bounded default is an SDK `post` over `TrySend`.
- **Step 2, typed outcome** (2026-11-11). `ostd::call_outcome::CallOutcome<T>` is `Reply(T) | Refused(Refusal) | Uncertain(Uncertainty)`.
  - `classify(IpcError, stale)` is a pure function. Refusals are only failures before acceptance: `Encode`, `Busy`, `Send`, `PreDispatchTimeout`, `Cancelled`. A stale binding turns a refusal into `NoBinding` and an uncertain result into `ProviderReplaced`.
  - `ServiceRef::call` and `LocalEndpoint::call` return it.
  - `CallOutcome::into_result` keeps the old `ViResult` codes exactly, so the 24 `ostd` client sites, 11 net-broker sites and 10 test-cell sites changed only by collapsing explicitly.
  - Deleted: `LocalEndpoint::new`, the tid-only path and `EndpointError::InvalidLocalTid`. `LocalEndpoint` is bound only.
  - Deleted: `ostd::dispatch` (`MessageHandler`, `run_service`), which had no callers.
  - Spec 20 §1 row updated.
- **Deviation from CP1 §3 ("post over `TrySend`"):** `TrySend` drops a message unless the target is already parked in `Recv`. A service reply posted that way would be lost whenever a raw-`Send` client has not yet reached its receive, which is the common case on SMP. The non-parking path is therefore `SendGather`/`sys_post` (202, existing). It queues into the target's bounded mailbox and fails when the mailbox is full. There is no ABI change; cells with an explicit allowlist must list `SendGather`.
- **Found while running lanes (pre-existing; not from this slice):**
  - **`qemu-phase03-pipeline-test.sh` had been broken since the Phase 09 dispatcher extraction.** It looked for the shutdown arm in `syscall/mod.rs` and now reads `dev::shutdown`. Fixed in this step.
  - **Possible kernel bug.** `local-service-lifecycle-x86` `x86_multi_source_wait_holds_on_two_cpus` fails intermittently with `CANCEL-RACE … delivered_twice=1|2`: the provider received one request twice. Old lane logs show it since at least the Phase 08 runs (`p08b`, `p09c`, `ipc1`). This is the intermittent failure previously recorded as "timeout". Root cause (read-only `haily-debugger` analysis, confirmed by a host test):
    - The kernel never delivered twice. A blocking `Recv` woken by `IpcSubmit` for a request that `IpcCancel` removed before the receiver ran resumed with an empty mailbox. `snapshot_resume` then returned `Wake { sender_tid: caller.current }`, so the syscall completed as a "message" from the previous caller over the unchanged buffer.
    - Every message reaches a receiver through its mailbox and every death through the death fields, so this wake is always spurious. `IpcSubmit` cancel and posts that `mask` does not admit both cause it.
    - The same path made a timed-out `RecvTimeout` return the previous caller's tid instead of 0.

  **Fix:** `syscall::spawn::resume_or_repark`. Under the lock that found the mailbox empty, it parks the receiver again with the same mask, buffer and deadline. It reports a timeout as sender 0 once the deadline has passed. `recv` and `recv_timeout` loop on it.

  **Host tests:** `a_receiver_woken_for_a_cancelled_request_parks_again` and `a_timed_receive_woken_with_nothing_parks_until_its_deadline`. Both fail when the re-park is disabled.
