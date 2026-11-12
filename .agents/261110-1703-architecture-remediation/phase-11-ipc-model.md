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

- [x] Call-site census, CP1 *(owner confirmed D1-D7, 2026-11-11)*
- [x] Typed outcome surface
- [x] Bounded send, `IpcReply` replies *(the `clippy.toml` rule is deferred to step 7; see Deviation Log)*
- [x] Registration flags, `fast_ipc` removed
- [x] Waits folded, states collapsed
- [x] Dead mechanisms removed
- [ ] Bound lookup only
- [ ] CP2

## Success Criteria

- [x] New x86 fixture: a client that never receives does not park the supervisor, config or compositor (each service continues serving a second client) — fails on the tree before this phase. *(`bench nonreceiving-client`; fails on `e4755d8b0`)*
- [x] Killing VFS mid-`Rename` returns an error to the caller within the deadline instead of wedging the task. *(host test on the real scheduler state; a QEMU kill cannot be timed, see Deviation Log)*
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
- **Step 3a, non-parking replies** (2026-11-12). New `ostd::ipc` surface in `ipc/post.rs`:
  - `post(tid, bytes)` queues into the target's mailbox (`SendGather`, see the deviation above) and never parks;
  - `Incoming::current(sender)` captures the request's operation at receive time, and `Incoming::reply` answers it with `IpcReply`, or posts when the request came by raw `Send`;
  - `reply_to(sender, bytes)` is for reply wrappers that did not capture: with no current operation it posts; with one it goes through the kernel's reply bridge, which checks that `sender` is the caller the operation belongs to;
  - `post_blocking` is the one sanctioned blocking send (input-source registration and HID events, CP1 §3).
  - **Deviation from CP1 §3 (`reply_current(bytes)` returning `NoOperation`):** every receive replaces the kernel's current operation (only VFS keeps it across masked receives), so a token read lazily inside a wrapper could belong to another caller and `IpcReply` would answer the wrong client. Services capture `Incoming` where they receive; wrappers use `reply_to`, which leaves the caller check to the kernel bridge.
  - Migrated: every blocking `sys_send` in `cells/services` (supervisor, config, compositor, net, vfs, kms, ai, ocel-js/pdf/quickjs), the block-driver replies (ahci, nvme, pvscsi, virtio-blk, virtio-scsi), dwc2 NIC replies and the lan9514 relay, init's hypervisor-exit relay, `kill`'s shutdown sentinel, the hypha tools and the hotswap demos. `AppContext::send` became `AppContext::reply`; `AppContext::send_msg` and `ActorCtx::send_msg` post; `ActorCtx::reply` replies.
  - Requests became posts where a receive masked to the same peer follows: kms silo (bounded, 8 ticks) and the VFS block router (unbounded; a dead driver is still caught by the masked-receive peer-gone check).
  - The compositor CONFIGURE_ACK state event is posted and the status is the reply, so the bridge can no longer turn the event into the reply (CP1 §1 possible misdelivery).
  - Not migrated in this step: `recovery-demo` (its `common::send` carries requests too; it moves with the request sites).
  - **The `clippy.toml` `disallowed-methods` rule is deferred** to the end of the request migration (step 7). Under `-D warnings` it would fail on the ~150 request and test sites that still call `sys_send`.
  - **Witness** `bench nonreceiving-client` (`cells/tests/bench/src/scenarios/nonreceiving_client.rs`, test `x86_services_do_not_park_on_a_client_that_never_receives` in `local-service-lifecycle-x86`). A probe sends to supervisor, config and compositor by raw `Send` and never receives again; the bench's own bounded call to the same service must get the right reply within 300 ticks. The lifecycle image now packages `/bin/supervisor` and `/bin/compositor` and boots with config features `input,supervisor,ai,ui` (`ai` is what launches config; `/bin/ai` is not packaged). PASS twice on the new tree. **Negative control:** the same witness on `e4755d8b0` (before this step) fails with `config parked on a client that does not receive`.
  - **Bridge ordering (CP1 §4.3)** has no QEMU witness: a CONFIGURE_ACK needs a live surface with a configure in flight, which only the UI lanes create. The ordering is fixed by construction: the event is posted and only the status goes through `reply_to`.
- **Step 3b, `Rename`** (2026-11-12). The kernel submits one exact operation to the live VFS for the caller (`async_ipc::submit_request` with `Request::Kernel`, the user path unchanged) and parks it in `IpcWait` until that operation is terminal (`take_kernel`). VFS death returns `PeerGone`, and a dispatched request expires after `OPERATION_TIMEOUT_TICKS`; both are errors. Before, the caller sat in `Recv { mask: vfs, deadline: None }` with no way out. `task::ipc_recv_kernel` had no other user and is deleted.
  - **Deviation:** CP1 §4.2 asks for a QEMU fixture that kills VFS mid-`Rename`. VFS answers a rename in microseconds and nothing can hold it between receive and reply, so the kill cannot be timed. Host test `a_kernel_submitted_request_terminates_when_its_provider_dies_mid_request` drives the same transitions on the real scheduler state: submit, dispatch (VFS took it), VFS death, `PeerGone`, operation gone. The existing raw-Rename witness in `srv-cellosfs` (sealed refusal, unsealed commit, identity) checks the live path.
- **Independent review of step 3** (`haily-reviewer`, read-only, on the uncommitted diff). Verdict: BLOCKED on two allowlist gaps. It found no path where `Incoming` or `reply_to` answers another client's operation. Dispositions:
  - HIGH, `app_entry!`/`service_entry!` base allowlist lacked `SendGather`, so `AppContext::reply`/`send_msg` posts were refused (hotswap demos, sdk-demo): **fixed**, `ostd::runtime::app_syscall_set` lists it.
  - HIGH, `backend-supervisor`/`backend-worker` allowlists lacked `SendGather` (actor calls and replies now post): **fixed**.
  - MED, supervisor hotswap waits counted 500 yields from when the snapshot/restore event was queued: **fixed**, the stash, ready and pause waits use a 500-tick deadline (`WAIT_TICKS`); an unreadable clock counts as expired.
  - MED, the Rename wait returned at once when another of the caller's operations was already terminal, turning it into a yield loop: **fixed**, `async_ipc::take_or_park_kernel` checks only the Rename operation and parks the caller in `IpcWait` otherwise.
  - LOW, unreachable `TooSmall` branch that would leak the operation: **fixed**, the reply buffer is typed `[u8; MAX_IPC_WIRE_PAYLOAD]`, so the branch is gone.
  - LOW, a request VFS never took returned `Unknown`: **fixed**, `PRE_DISPATCH_TIMEOUT` returns `TryAgain` (the ABI still collapses both to the generic error).
  - LOW, `ipc_post_nonblock` accepted a retiring or terminated target: **fixed**, it fails as a blocking send does (`ipc_target_gone`).
  - LOW, Deviation Log said the block router's receive is bounded, and the actor docs still said replies block: **fixed**.
- **Manual lanes the review asked for**, each also run on `e4755d8b0` (before step 3) as the baseline:
  - `native-workload`: does not build on either tree (`supervisor --features hostile-backend-recovery` lacks `sender_has_exact_name`). Pre-existing.
  - `actor-supervisor`: fails identically on both trees (`SpawnFromPath ... /bin/backend-worker error=NotFound`, so no child starts). Pre-existing staging defect; the actor call/reply change has no QEMU witness.
  - `hotswap-smoke`: baseline 13/13. On the new tree the swap itself always succeeded, but `supervisor_hotswap_preserves_demo_state` failed in 2 of 4 runs because the CLI's success line arrived split (`USER: ...service '` / `USER+ hotswap-demo' to ...`). The CLI wrote the line in five writes, and a kernel log record landed between them now that the reply arrives as soon as it is queued. **Fix:** `hotswap` writes the line in one write (`emit` joins the parts into a 512-byte buffer).
- **Step 4, registration flags and `fast_ipc` removed** (2026-11-12). D1 as confirmed:
  - `api::syscall::service_flags { RETAINS_OUTER_CALLER, OWNER_WATCH, ALL }`; `RegisterService` `a2 = flags`. Only trusted init or a `SupervisorCap` holder may set them (`PermissionDenied`), an undefined bit is `InvalidInput`, and a `tid=0` self-registration with flags is `InvalidInput`.
  - Registry rows carry the flags (`ServiceEntry` is now one struct with `flags` and `paused`). A hot-swap commit hands them to the replacement; a plain re-registration states its own.
  - `service_registry::retains_outer_caller(cell, generation)` replaces `is_registered_vfs_cell` at its 9 production sites (caller-context retention in `tcb`, the async-reply clear, `GrantSlice` lease lookup and install, `ResolveCellOwner` 244, `ResolveCellOwnerRecord` 247, `ResolveCellImage` 441). `grants_owner_watch(cell, generation)` replaces the VFS-or-`OWNER_WATCH_SERVICES` rule. Both match Cell identity *and* generation; the old VFS rule matched the CellId only.
  - Deleted: `kernel/src/fast_ipc.rs`, `libs/ostd/src/fast_ipc.rs`, `cells/services/vfs/src/fast_handler.rs`, `OWNER_WATCH_SERVICES`, the `VFS_HANDLER_WRITE` rank, VFS's `AccessTable::can_read_fast` (only the fast handler called it) and their tests. A `block_io` launch route no longer makes a Cell "the VFS"; `commit_launch_routes` lost its `cell_id` argument.
  - Policy moved to the namespace owner: `ostd::service::namespace_flags` (VFS → both flags; NET, COMPOSITOR, XE_GPU → `OWNER_WATCH`), pinned by the tripwire `namespace_flags_are_an_explicit_table`. Init registers configured services and the bootstrap VFS with it; the supervisor's hot-swap rollback re-registers with it. Every other caller passes 0.
  - **Decision: per-flag pause semantics.** `OWNER_WATCH` holds only while the row is Active (as the table rule did: a quiesce grants no new watch). `RETAINS_OUTER_CALLER` also holds while Paused, so requests VFS already took finish with their caller. Consequence: a paused VFS can no longer start a new owner watch, which the CellId rule allowed.
  - **Decision: registry lock on the receive path.** `begin_receive_context` now reads the registry (rank SCHEDULER → SERVICE_REGISTRY) on every wildcard receive instead of one atomic load. Every site already holds the global `SCHEDULER` lock, so it adds no new serialisation point; `set_received_request_context` checks the cheap `caller.current` first.
  - **Deviation from CP1 §3 ("`fast_ipc` deleted on both sides"):** `libs/api/src/abi/fast_ipc.rs` (`TrustedHandle`, `VfsCell`) stays. It has no in-tree user, but removing a `libs/api` module is a Law-1 delta CP1 did not list; it is proposed in CP2.
  - Fixtures: the atomic-publication baseline registers a flagged private row for its VFS task (the snapshot now compares row flags too); the VFS lifecycle selftest grants its VFS tasks a flagged private row instead of `set_vfs_handler_cell`; the kernel owner-watch matrix flags its table rows and proves a flagged VFS row grants by flag.
  - Generated: `docs/generated/lock-ranks.md`, `docs/generated/mutable-globals.md` (three `fast_ipc` globals removed from `scripts/mutable-globals.toml`), `kernel/residents.toml`.
  - Evidence: kernel host tests 629/629 (new `registration_flags_follow_the_row`; `register_service_refuses_well_known_hijack_by_a_spawn_only_cell` now also proves the flag refusals and that a flagged row grants), `ostd` 114/114 (tripwire), `service-vfs`/`app-init`/`supervisor`/`api` host tests green; `x86_64` workspace clippy (with `test-hooks`) reports nothing in the touched files (its 7 findings are pre-existing, in compositor, `ostd::display` and `kernel/hypervisor`); `local-service-lifecycle-x86` 4/4; RV64 `atomic-publication` pass; RV64 `boot` subset 12/12 including `vfs_lifetime_selftest_passes`, `hot_migration_state_transfer_works`, `block_io_denied_non_vfs` and the FAT/VFS write tests (init's flagged VFS registration is what VFS's grant leases now rest on).
- **Step 5, one parked state; the legacy waits share the completion sweep** (2026-11-12). Owner choice: fold the wait state and wake paths, not the queue (the completion engine reserves a queue slot per wait, which would add `TryAgain` to WaitForEvent/WaitIrq, and serves only x86 MSI lines).
  - `TaskState` is `Ready | Running | Blocked { wait: WaitKind, deadline } | Terminated | Retiring | Frozen`. `WaitKind` is `Sleep | Send | Recv | PipeRead | PipeWrite | Futex | Join | Completion { source, delivery }`. The 13 parked variants are gone; `wake_deadline()` is the `deadline` field and `waits_for_events()` is "a completion wait other than `IpcWait`", which is what the Phase 10 deadline queue and event list key on.
  - D2/D3: `WaitForEvent`, `IpcWait` and `WaitIrq` park as `WaitKind::Completion` with `Delivery::{EventMask, IpcTerminal, IrqLine}`; `WaitCompletion` uses `Delivery::Queue`. The sweep has one completion arm (`completion_wait::sweep_wake`), the IPC terminal wake is one rule ("any completion wait naming IPC"), and the kernel `Rename` wait parks the same way. Results and timeout conventions are unchanged per delivery and pinned by `the_sweep_keeps_each_completion_delivery_result`.
  - Deleted: `TaskState::Polling`, `SyscallFuture`, `Task::pending_future`, the dummy waker and the hart-0 polling pass (nothing entered `Polling`).
  - `ProcessInfo` encoding unchanged (D6): every `Blocked` is 2.
- **Step 6, retired opcodes and dead mechanisms** (2026-11-12). D4, with the census corrections the owner confirmed:
  - Retired as `[[reserved]]` rows (number and allowlist bit never reused; the table parser refuses a reserved number or bit that a live row uses): `Call` 2 (bit 4), `Reply` 3 (bit 3), `Exec` 6 (bit 35), `AudioPlay` 218 (bit 47), `NetTx` 310 (bit 27), `NetRx` 311 (bit 28). `ViSyscall` has no variant for them and the kernel decodes them to nothing (`UnknownSyscall`); negative control `retired_numbers_decode_to_nothing`.
  - **Deviation from CP1 D4 (census error): `Yield` 104 stays.** It is the only yield (`ostd::sys_yield`, posix); CP1 listed it as a legacy alias.
  - **Deviation from CP1 §2 D4 ("nothing in `libs/` or `cells/` calls them"):** the net service's "NIC driver unavailable: legacy frame path" called `NetTx`/`NetRx`, and `audio-demo` called `AudioPlay`. Owner decision: retire anyway. With no NIC driver the net service now moves no frame (an `L2Send` is refused; a frame queued while a driver was live is answered as failed), which is what the kernel arms already did (`NetTx` discarded, `NetRx` never returned data). `audio-demo` is deleted with its image, catalog, launch-test, hypha spawn-allowlist and unsafe-allowlist rows. `ostd::sys_exec`, `sys_audio_play`, `sys_net_tx`, `sys_net_rx` are deleted; the base app allowlist drops `Reply`, the network extra drops `NetTx`/`NetRx`, and 27 cell `declare_syscalls!` lists drop the retired names.
  - **Surprise:** `map_syscall`'s raw fallback mapped 3 to `SetTimer`, unreachable while 3 was `Reply`; it would have revived 3 as `SetTimer` (bit-less, i.e. always permitted). Removed; the negative control covers it.
  - Dead kernel mechanisms removed: the five `[[internal]]` variants (`Lend`, `BorrowRead`, `BorrowWrite`, `Grant`, `Map`) with their handlers, `task::ipc_{lend,borrow_read,borrow_write,grant,map,reply}`, `Task::{leases, next_lease_id, grant_table, next_grant_id, cluster_mode, cluster_id}` and the `Lease`/`LeaseAttributes`/`GrantEntry` types. `TaskLaunchState` stops carrying the cluster id; spawn still reads the cluster mode for its RealTime refusal. `[[internal]]` support in the table parser is replaced by `[[reserved]]`. `allowed_drivers` and `pku_key` stay (copied to threads), as CP1 said.
  - With `Call` gone no api row shares `ServiceLookup` with raw 100, so `in_kernel_gate_matches_the_trap_gate` now requires the two gates to agree everywhere.
  - Golden: `syscalls.golden.json` changes in rows 2, 3, 6, 218, 310, 311 (gone) and 100 (`kernel_vi` no longer `Call`); `catalog.generated.json` loses `/bin/audio-demo`.
- **Evidence, steps 5 and 6** (lanes on each step's own tree): `local-service-lifecycle-x86` 4/4; RV64 `atomic-publication` pass; RV64 `boot` subset 15/15 (shell, FAT/VFS writes, `vfs_lifetime_selftest_passes`, hot migration, DHCP, TCP send/recv/listen/accept, curl, httpd); AArch64 boot to shell on `virt` (1 CPU), `virt-vhe` and `virt-el2-novhe` (2 CPUs). Kernel host tests 630/630; `x86_64` and RV64 workspace clippy report nothing in touched files.
- **Step 7, bound lookup and bounded requests** (2026-11-12).
  - **7a, D5.** `ostd::service::lookup` returns the `ServiceBinding` (`LookupServiceBound`); `ostd::service::provider_tid` is where to send, from that binding. `ostd::syscall::sys_lookup_service` and the kernel's `LookupService` (206) are gone; 206 is a `[[reserved]]` row without a bit, because bit 37 stays with `LookupServiceBound` (it was shared), so an image that declared `LookupService` still resolves bindings. The ~150 tid-only call sites, the posix VFS client and 83 cell allowlists moved. The five block drivers and input's HID-producer check authorise senders with `sender_is_live_provider`.
  - **7b, requests.** No blocking `sys_send` is left outside `libs/ostd` and `cells/tests` (85 sites). Requests are exact operations with tick budgets (`ostd::ipc::call_bytes`, new, for byte protocols; `service_call*_bounded` for postcard); one-way messages are `post`. The root `clippy.toml` forbids `ostd::syscall::sys_send` (`disallowed-methods`); `libs/ostd/clippy.toml` and `cells/tests/clippy.toml` exempt those trees. Negative control: a `sys_send` added to `shutdown.rs` is reported. Done by three parallel workers (SDK; net-tools; remaining cells) under one contract, integrated and compiled here.
  - **Behaviour changes, each deliberate:**
    - vimio/vipolling `TcpReady { wait: true }` now really waits (NET defers a readiness wait only for an exact operation; a raw send got `Err(0xFD)` at once, so both libraries polled). Any mailbox message still ends the wait, as the old wildcard receive did.
    - Compositor events that used to be routed inside `create_surface`/`stage_grant` stay in the mailbox and are routed by the next `poll_surface_events`/`input::poll_events`, in the same order.
    - nc/httpd accept loops stop on `PeerGone` instead of spinning.
    - wasm `vi.send` posts and returns -1 when the target is gone or its mailbox is full (it always returned 0).
    - recovery-demo: its own protocol answers through captured `Incoming`; the Stall path keeps Staged answering Mutate and Done/Failed answering the later Go through the Go's operation. A failed `Restored` post stops the worker ("authority unreachable") instead of serving unpublished.
    - hypha core empties its mailbox before the approval prompt (`discard_typed_ahead`, needs `TryRecv`): the old wildcard receive used to discard type-ahead, which kept typed-ahead keys from approving a tool call.
  - **Found while integrating:** `AppContext::reply` answered through `reply_to`, which relies on the kernel's record of the current request; a handler that did any receive before replying (the hypha gateway's net/TLS path) would post its answer and leave an exact-operation caller to time out. `AppContext` now captures `Incoming` at every receive and `reply(tid, …)` answers through it.
  - Cells that now post through SDK paths (focus, surface events) list `SendGather`: desktop, fb-console, files, hypha core, ocel, terminal, viui-demo, lua, phase04-terminal-driver, cellbox.
- **Independent review of steps 4–7** (`haily-reviewer`, read-only, `c6d519e7f..d43c9acef`). Verdict: BLOCKED on one HIGH. Steps 4–6 clean (every non-test `Blocked` site, the sweep rules, flag authority and lock order, retired decode paths checked). Dispositions:
  - HIGH, bounded calls read the scheduler clock (`GetTime`, bit 25) before submitting, and 13 cells now making them lacked the bit, so every call failed with `InvalidOperation` (fb-console exited at boot): **fixed**, `GetTime` added to nc, httpd, mqtt, hypha core and llm-gateway, fb-console, silo-test, the three recovery-demo cells, https-demo, viui-demo and robot-demo.
  - MED, wasm `vi.send` posts without `SendGather` in the wasm cell's list: **fixed**.
  - MED, posts let one busy client (damage every frame) fill a service's 64-slot mailbox and crowd out other clients: **fixed in the kernel**, a Cell may have at most `POST_QUOTA_PER_SENDER = 16` posts queued in one target's mailbox (`Busy` beyond); kernel senders (UART relay, power button) keep the whole queue. Host test `a_cell_posts_into_a_bounded_share_of_a_mailbox`.
  - LOW, hypha `discard_typed_ahead` unbounded on a persistent receive error: **fixed** (at most 64 receives).
  - LOW, `begin_receive_context` reads the registry on every wildcard receive: **fixed**, skipped when there is no caller context to drop.
  - LOW, stale `sys_lookup_service` doc comments: **fixed**.
  - LOW, nc/httpd accept loops retry after `Indeterminate` (a connection accepted just before expiry is lost): not changed; NET answers an accept in the same loop turn. Residual in CP2.
  - LOW, compositor input frames sent by `sys_try_send` while a client waits on a compositor call are dropped (one compositor turn): not changed; residual in CP2.
  - LOW, the kernel accepts any defined flag on any row from a `SupervisorCap` holder: not changed; flag policy is the namespace owner's by design (CP1 D1), and only `/bin/supervisor` holds the cap. Residual in CP2.
- **Digest re-pins (CP1 §4.5).** `check-multi-source-wait-law1-digests.sh`: the "IRQ wake on a pending snapshot line" and "deadline keys on TIMER" literals moved, unchanged, from the scheduler sweep into `completion_wait::sweep_wake`; the script now pins them there plus the sweep's call. `check-lookupservicebound-law1-digests.sh`: "206 kept, bit 37 shared" becomes "206 is a bit-less reserved row, bit 37 on 429" (D5), and the test name follows. Negative controls: a bit on reserved 206, or flipping the TIMER rule, fails the scripts.
