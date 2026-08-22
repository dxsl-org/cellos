---
title: "CELLOS-VFS-SMP-006: Cell owner-lifetime contract"
description: "Plan the kernel-to-VFS lifetime binding needed when bounded reusable CellId and monotonic task TID differ."
status: completed
completion_state: "CELLOS-VFS-SMP-006_CLOSED_VERIFIED_RV64"
verification_status: verified
priority: P1
created: 2026-08-22
tags: [kernel, vfs, ipc, lifecycle, smp, security]
---

# CELLOS-VFS-SMP-006 — Cell owner-lifetime contract

## Scope and terminal condition

This plan repairs the confirmed owner-lifetime defect, not the already-completed masked VFS request/reply receive repair. It MUST preserve the existing `CallerIdentity` 32-byte trailer, existing syscall numbers and argument layouts, Cell ownership semantics, and the Phase 07 final atomic-publication boundary. It MUST NOT derive a Cell owner from `CellId`, treat `sender_tid` as an owner, weaken VFS ownership checks, or add a client-side workaround.

Terminal status is **`CELLOS-VFS-SMP-006_CLOSED_VERIFIED_RV64`**. The owner-lifetime implementation and required RV64 lifecycle evidence are complete. `ATOMIC_PUBLICATION_PREREQUISITE_COMPLETE / PHASE07_BLOCKED` remains the separate narrow atomic prerequisite marker; it does not qualify full Phase 07 or Tier 2.

## Confirmed causal chain

`kernel/src/task/launch.rs` allocates a monotonic task TID, while `kernel/src/memory/cell_quota.rs::QuotaReservation::reserve_next` allocates the lowest reusable bounded CellId. Therefore a governed cell can have `tid != CellId`, and a reused CellId names neither an old nor a new TID.

The defect recorded durable VFS state by `Caller(cell_id, generation)` while `cells/services/vfs/src/manager/owned_state.rs` registered `NotifyOnExit(cell_id)` and indexed its watch map by CellId. The kernel correctly treated that nonexistent task as dead, queued a synthetic death, and `cells/services/vfs/src/main.rs` then purged the caller's `PendingTable`, directory entries, file handles, and ordinary handles. The resulting two-hart `40 PASS, 10 FAIL` baseline correlated with those durable-state paths. Using `Caller.sender_tid` would instead purge the same Cell's state when any worker exits and was explicitly rejected.

## Required authoritative contract

### Terms

- **Principal**: `(CellId, cell_generation)`. This remains the key for VFS authorization, quotas, directory entries, pending reads, ordinary handles, and file handles.
- **Root/owner task**: the governed task that first publishes that principal. Its monotonic TID is the lifetime endpoint for the Cell, even when CellId differs.
- **Thread**: a task subsequently created in the root's Cell. It has a distinct sender TID but inherits the root CellId and cell generation. A thread is never a Cell lifetime endpoint.
- **Owner record**: `CellOwner { cell_id, generation, root_tid }`, available only while that exact Cell generation is live. `root_tid` is stable for the record's lifetime and is never inferred from CellId or sender TID.

### Kernel representation and publication

1. Add a scheduler-owned, fixed-capacity `CellId -> CellOwner` registry indexed by the existing bounded quota CellId range. It MUST not be a task-table lookup, a dynamically allocating map, or a `CellId == tid` convention. A slot is empty, live for one exact generation/root TID, or retiring; generation zero and CellId zero are invalid.
2. `governed_spawn`/`launch::spawn_prepared` constructs the owner record from the reserved CellId, the newly published task's minted generation, and the assigned monotonic TID. During the existing final scheduler-locked, infallible Phase 07 commit, fully configure the task, publish the live owner slot and task-table entry together, perform only established infallible success side effects, then put the task on a ready queue last. No fallible lookup, allocation, route validation, ELF work, quota reservation, or injected failure point may occur after that commit begins.
3. A failed or denied launch MUST leave no owner-slot entry, no member record, no VFS-visible identity, no quota reservation, and no ready task. It may leave gaps in a monotonic generation source only if that source is intentionally specified as uniqueness-only; it MUST never reuse a generation for a CellId. Existing Phase 07 rollback assertions remain authoritative.
4. `spawn_thread` resolves the parent principal through this registry and copies the exact CellId/generation/root association. It MUST reject a missing or retiring record. It MUST NOT recover the generation by `tasks.get(CellId as usize)`.
5. Provide one canonical scheduler helper used by IPC attestation, VFS owner-watch authorization, directory provenance, launch/thread creation, and all teardown paths:
   `resolve_live_cell_owner(cell_id, generation) -> Option<CellOwner>`.
   It succeeds only when the slot matches both fields, the root task is still the recorded root and live, and the root task itself still carries the same principal. Any mismatch, generation zero, CellId zero, retiring slot, or missing root returns `None`/deny.

### Root and thread exit rules

1. Exit of a non-root thread removes only that task's task-local state, sender-specific grant/pin/IOMMU state, and generic task watchers. It MUST retain the Cell owner slot, quota/resource ownership, Cell generation, and VFS owner watch. A VFS watch is registered only for `root_tid`, so a worker exit cannot purge Cell-owned VFS state.
2. Exit, force-exit, fault, watchdog, heartbeat termination, or hot-swap retirement of the root is Cell termination. Under the scheduler lock, transition its owner slot to `Retiring`, prohibit new members/attestations, and remove/terminalize every member of that exact owner record. On SMP, remove local/remote ready work and use the existing cross-hart scheduling/interrupt acknowledgement path to prove no member can still run before quota/resource release or CellId reuse.
3. Only after member quiescence may the kernel remove the owner slot and release CellId-keyed quota/capability/MMIO state. The root's death notification is emitted exactly once from the normal task-death funnel; it is the VFS cleanup event. Existing generic watches on individual worker TIDs retain their task-level semantics but are not Cell-owner watches.
4. Centralize the owner-vs-thread decision so no exit path releases a CellId merely because one task carrying it exits. Migrate voluntary `Exit`, `ForceExit`, fault termination, scheduler heartbeat/watchdog paths, and `cell::hotswap::exit_task_internal`; do not leave duplicate “mirrored cleanup” branches.

### Retirement-causality baseline

The owner-lifetime contract retains the independently justified SMP retirement chain: per-hart boot contexts, secondary-hart SIE enablement before schedulable publication, selected/executing ownership pins, incoming-side completion epochs, task-to-idle attribution clearing, and RV64 pre-selection SIE masking with the complete outgoing `sstatus` restored after late `s11` restoration. `retirement_selftest.rs` is the source-level regression owner for this chain.

Destination-`tp` Context binding and deferred requeue are not part of this contract or the next init-fault baseline. The recorded `tp` A/B did not observe a mismatch before the exact fault, and deferred requeue is not required to establish root/member retirement quiescence. Either mechanism requires a separate falsifiable control/treatment proof before it can be retained.

### ABI-preserving owner lookup and watch capability

The current `CallerIdentity { cell_id, generation, sender_tid }` trailer stays exactly 32 bytes and `sender_tid` stays diagnostics/transport identity only. The existing `Recv`, `NotifyOnExit`, and `QueryDirHandles` ABI contracts remain unchanged.

Add an append-only, VFS-provider-only syscall pair and fixed API record; do not overload the trailer, request frames, or existing syscall arguments:

- `ResolveCellOwner(cell_id, generation, out)` returns the kernel-attested `CellOwner` only when the caller is the registered VFS Cell and its current receive context has the same principal. It returns a denial/no record on stale, dead, arbitrary, kernel, or mismatch inputs.
- `WatchCellOwner(cell_id, generation)` validates the same receive context, resolves the live owner, and atomically records a death subscription for the returned `root_tid` while the scheduler lock still proves the owner live. It returns an opaque one-shot watch token plus root TID. A paired `CancelCellOwnerWatch(token)` removes only that exact live subscription; cancellation is idempotent after delivery.

The watch operation, rather than a resolve-then-legacy-`NotifyOnExit` sequence, closes the root-death race between lookup and registration. Kernel subscription entries need token identity in addition to watched/watcher TIDs so cancellation and duplicate registration cannot remove an unrelated watch. Death delivery carries the root TID through the existing receive mechanism; VFS matches that TID to its recorded `CellOwner`/token and never converts it to a CellId. A queued death racing a cancellation is harmless because its token/owner record is no longer present in VFS and it cannot match a successor generation.

`NotifyOnExit` remains for supervisors and exact task watches. Its VFS exception (`allows_current_caller_owner_watch`) is deleted after all VFS callers use `WatchCellOwner`; this is a clean cutover, not a second owner-watch scheme.

### VFS ordering and teardown

1. On every attested VFS request that can create durable state, parse the unchanged trailer into `Caller`, then obtain the owner record through `ResolveCellOwner`/`WatchCellOwner`; failure is `Err(3)` before dispatch. `sender_tid` is retained only for replying to the actual sender.
2. `VfsManager` stores a `WatchedOwner { principal: Caller, root_tid, token }`, keyed/matched by the full principal and root TID—not `BTreeMap<CellId, Caller>`. Principal equality remains `(cell_id, generation)`; root TID is lifecycle evidence, not a replacement authority key.
3. Dispatch may create `PendingHandle`, `DirHandle`, `FileHandle`, and current ordinary handles only after a live owner record is obtained. If watch registration fails, immediately purge only that exact principal's state, cancel any provisional token, and encode the deny response before replying. Never purge by sender TID.
4. On a successor generation observed for a reused CellId, purge/cancel the predecessor's exact principal before registering the successor. A stale predecessor notification may only miss its removed watch record; it MUST NOT touch successor state.
5. On a death delivery without a valid caller trailer, check it only as a root-death candidate against the root-TID/token watch record. A match purges that exact principal's directories (and dependent file handles), files, ordinary handles, pending reads, and watch record, then cancels/consumes the token. A non-match remains unattributed and is denied/ignored; it never triggers broad cleanup.
6. Keep `GLOBAL_VFS` unlocked across all syscall registration/cancellation and response send operations. Make provisional record changes under the VFS lock, perform kernel calls after unlocking, and reacquire to commit or roll back only the exact record. This preserves the existing blocked-`sys_send` deadlock avoidance.
7. `dir_inherit::attestation_for` resolves its CellId through the owner registry before reading the root's immutable inherited-directory record. `QueryDirHandles` remains ABI-compatible: a stale caller may receive a successor record, but VFS's existing `(cell_id,generation)` comparison rejects it. No thread lookup may assume `tasks[CellId]`.

## Exact caller and source inventory

### Kernel identity, lifecycle, and ABI

- `kernel/src/task/launch.rs` — governed launch assigns TID/CellId and performs Phase 07 publication; publish/remove the owner slot at the correct commit boundary.
- `kernel/src/task/scheduler.rs` — scheduler owner registry, member/root lifetime transitions, `spawn_thread`, `exit_task`, death subscriptions, heartbeat/watchdog, SMP quiescence ordering.
- `kernel/src/task/tcb.rs` — explicit immutable root association or task membership fields; generation construction and test-hook snapshots.
- `kernel/src/task.rs` — `sender_context`, direct IPC delivery, `terminate_current_cell_on_fault`, and common teardown dispatch.
- `kernel/src/task/syscall.rs` — `attested_identity_of`, trailer writes, receive delivery, syscall enum/decoder/allowlist policy, existing `NotifyOnExit` cutover, `Exit`, `ForceExit`, `QueryDirHandles`, VFS grant context, and privilege checks.
- `kernel/src/task/dir_inherit.rs` — root-record directory attestation without `tasks[CellId]`.
- `kernel/src/cell/hotswap.rs` — replace duplicated Cell cleanup with root-aware common retirement while preserving hot-swap atomicity.
- `kernel/src/memory/cell_quota.rs` — bounded reusable CellId reservation is the registry capacity/slot-reuse authority; no CellId-to-TID assumption.
- `kernel/src/loader.rs`, `kernel/src/loader/governed_spawn.rs`, `kernel/src/loader/atomic_publication_tests.rs`, and `kernel/src/loader/atomic_publication_tests/*` — verify owner-slot setup/rollback is within Phase 07 publication and failure injection corpus.
- `libs/api/src/abi/caller_identity.rs` — retain layout and document why root identity is a separate syscall record.
- `libs/api/src/abi/syscall.rs` and its syscall tests — append new syscall IDs/permission semantics without renumbering existing values.
- New API ABI module (for example `libs/api/src/abi/cell_owner.rs`) — fixed `CellOwner` and token encoding/decoding, bounds, zero rejection, and round-trip tests.
- `libs/ostd/src/syscall.rs` — safe wrappers for resolve/watch/cancel; `libs/ostd/src/fast_ipc.rs` remains trailer-only unless a fast path starts creating durable state.

### VFS service and persistent owner state

- `cells/services/vfs/src/main.rs` — attested receive, owner lookup/watch/cancel calls, death handling, lock-drop ordering, response rollback.
- `cells/services/vfs/src/caller.rs` — preserve principal-only equality and make the root-versus-sender distinction explicit.
- `cells/services/vfs/src/manager.rs` and `cells/services/vfs/src/manager/owned_state.rs` — replace CellId-keyed watches with exact principal/root/token records and exact cleanup.
- `cells/services/vfs/src/dir_admission.rs`, `cells/services/vfs/src/dirs.rs`, and `cells/services/vfs/src/dirs/{bind,lifecycle,revoke}.rs` — preserve generation-based directory attestation and revoke only exact predecessor/successor state.
- `cells/services/vfs/src/{dispatch,dispatch_dirs,dispatch_file_handles}.rs`, `pending.rs`, `handle_table.rs`, and `file_handles/{table,owner_counts,selftest,tests}.rs` — audit durable-state creation and exact principal cleanup; no data structure may key ownership by sender/root TID instead of `(CellId,generation)`.
- `cells/services/vfs/Cargo.toml` — retain `test-hooks` wiring if owner-lifetime service self-tests are added.

### Test, image, and status wiring

- `kernel/src/task/vfs_lifecycle_selftest.rs` — replace the accidental `CellId == root TID` fixture with intentionally disjoint/reused CellIds and add kernel lifecycle/failure cases.
- `kernel/src/task/ipc_pending_selftest.rs`, `kernel/src/task/thread_cap_selftest.rs`, and `kernel/src/task/thread_quota_selftest.rs` — extend only where needed to prove thread inheritance and worker exit do not retire the Cell.
- `cells/services/vfs/src/file_handles/selftest.rs`, `cells/services/vfs/src/manager/tests.rs`, and module-local tests — add pure VFS watch-token/principal/reuse tests.
- `cells/tests/vfs-test/src/main.rs` and `cells/tests/vfs-test/src/dircap.rs` — retain masked service-TID RPC; add explicit owner-lifetime behavior markers only through a real test-hooks orchestration path, never a synthetic client-only success.
- `scripts/build-test-hooks-cells.ps1` and `scripts/build-test-hooks-ci.sh` — include any new test-hooks fixture/cell only if the implementation requires one; preserve signing and embedded image completeness checks.
- `tests/integration/tests/vfs-quota.rs` — retain the one-hart contract and add only deterministic owner-lifetime markers that are valid on one hart.
- `tests/integration/tests/vfs-smp.rs` and `tests/integration/Cargo.toml` — two-hart acceptance owner; retain the static no-wildcard check and require root-lifetime markers, not only a total pass count.
- `docs/system-architecture.md`, `docs/roadmap/open-risk-register.md`, `docs/roadmap/current-focus.md`, `.agents/260822-phase07-atomic-publication/plan.md`, and `.agents/260821-0642-app-tiers-completion/{plan.md,phase-07-tier2-native-domain.md}` — update only after passing evidence; preserve the blocked status until then.

## Phased implementation plan

### Phase 1 — Establish the kernel root-lifetime primitive

**Files:** `kernel/src/task/{scheduler.rs,tcb.rs}.rs`, `kernel/src/task.rs`, `kernel/src/task/launch.rs`, `kernel/src/memory/cell_quota.rs`, applicable `loader/*` atomic-publication files.

1. Model the fixed-capacity owner slot and task membership/root fields. Make the root record visible only after fully initialized task publication and make it absent on every denied attempt.
2. Replace all `tasks.get(CellId as usize)` root assumptions, beginning with `spawn_thread`, `sender_context`, and directory attestation, with `resolve_live_cell_owner`.
3. Route root versus worker teardown through a single scheduler decision/result so quota/resource/reuse cleanup occurs once for a root only. Convert every listed death origin to that result.
4. Add SMP retirement state plus remote-hart quiescence acknowledgement before releasing the CellId slot. Keep the SCHEDULER/death-subscriber lock order documented and unchanged.
5. Extend atomic-publication failure checkpoints around owner-record publication and rollback without adding fallible work to the final commit.

**Exit criteria:** disjoint CellId/root-TID publication works, a worker inherits the root principal, a worker exit leaves the owner record live, all root death paths retire the principal exactly once, and no failed launch leaves a live record.

### Phase 2 — Add privileged owner attestation/watch and migrate VFS

**Files:** `libs/api/src/abi/{syscall.rs,cell_owner.rs,caller_identity.rs}`, `libs/ostd/src/syscall.rs`, `kernel/src/task/syscall.rs`, `kernel/src/task/scheduler.rs`, `cells/services/vfs/src/{main.rs,caller.rs,manager.rs,manager/owned_state.rs,dir_admission.rs}`, plus affected VFS owner tables.

1. Append the fixed owner-record and opaque watch-token ABI; add wrappers and kernel decoder/authorization. Do not alter `CallerIdentity`, `Recv`, `NotifyOnExit`, or `QueryDirHandles` bytes or numbers.
2. Implement resolve and atomic watch registration under the scheduler lock. Bind access to registered VFS plus the exact current attested principal. Implement exact token cancellation and one-shot delivery behavior.
3. Migrate VFS request processing to resolve/register before durable state can persist; add provisional commit, cancellation, and exact rollback sequencing with `GLOBAL_VFS` unlocked around syscalls.
4. Replace `watched_owners: BTreeMap<CellId, Caller>` and generic unattributed `sender -> CellId` cleanup with root-TID/token matching. Remove the VFS `NotifyOnExit` exception after all service paths use the new API.
5. Move directory provenance resolution to the owner registry; retain VFS's existing generation comparison as the stale/reuse fail-closed boundary.

**Exit criteria:** no VFS path passes a CellId to legacy `NotifyOnExit`; a live worker request owns state under its principal and watches the root; root death removes only that principal; worker death and unrelated/unattributed IPC cannot remove it; a reused CellId cannot reach predecessor state.

### Phase 3 — Failure injection and two-hart proof

**Files:** kernel lifecycle/atomic self-tests, VFS self-tests, test-hooks build scripts as needed, `cells/tests/vfs-test/src/*`, `tests/integration/tests/{vfs-quota.rs,vfs-smp.rs}`, and status docs after evidence.

1. Add deterministic kernel fixtures where `root_tid` and CellId are deliberately different, a thread shares the principal, and the CellId is released/reused by a successor with a different generation. Do not use `CellId(tid)` fixtures.
2. Add VFS tests that create a `PendingHandle`, directory hierarchy, file handle, and ordinary handle for the predecessor; prove worker exit preserves all four, root death reaps all four, and successor state survives queued/stale predecessor notification.
3. Inject failures at: CellId reservation/prepared launch, immediately before final owner-slot publication, owner-record rollback, owner lookup mismatch, watch authorization denial, root dies before watch, root dies immediately after watch, cancellation racing delivery, and root teardown while a worker is assigned on hart 1. Each assertion must prove absence/exactness, not merely an error return.
4. Build/sign the test-hooks image through both supported script paths. Run the one-hart VFS contract to preserve ordinary behavior, then run the dedicated `QemuRunner::boot_rv64_smp(..., 2)` integration. The two-hart runner MUST observe hart 1 online, AP-00 through AP-15 including AP-13, both atomic terminal markers, the root-lifetime markers, `[vfs-test] ALL TESTS PASSED`, and no `[FAIL]`/failure summary. Update any historical literal `84 PASS` only to the verified new count, or replace it with named lifecycle markers plus zero failures; never falsely retain an obsolete total.
5. Only after successful evidence, update the risk register, roadmap/architecture blocker wording, and dependent Phase 07/08 plan references. Do not claim a Phase 07 regression or close Phase 07/Tier 2 as a side effect.

## Two-hart acceptance matrix

| Scenario | Injection/placement | Required observation |
|---|---|---|
| Disjoint allocation | root on hart 0, CellId from reusable slot differs from TID | resolve/watch returns root TID; VFS durable state remains reachable by its worker |
| Worker exits | worker that sent request exits on either hart | no root-death event; pending/dir/file/ordinary state persists for another thread of the same principal |
| Root exits with worker active remotely | root exits on hart 0 while worker is runnable/running on hart 1 | Cell enters retiring; remote member quiesces before slot release; exactly one root watch purges exact VFS principal |
| Reuse after root death | allocate successor in released CellId slot | successor has new generation/root TID and cannot access predecessor state; predecessor death cannot purge successor state |
| Pre-watch death | root dies between VFS owner resolution and registration | atomic watch fails/synthetic terminal result; VFS returns deny and creates no durable state |
| Post-watch death | root exits immediately after successful registration, including receiver not parked | queued/deferred root event matches token and purges only predecessor once |
| Cancel/delivery race | rollback or generation replacement races root exit | no leaked subscription, no duplicate purge, no cleanup of successor |
| Attestation abuse | VFS asks for stale/mismatched/arbitrary CellId+generation; non-VFS asks too | kernel denies; no owner TID disclosure and no subscription |
| Atomic launch rollback | every relevant governed failure checkpoint | no owner slot, no task/ready route, no CellId leak/reuse confusion; Phase 07 invariants still pass |
| Baseline VFS and SMP | one hart then `-smp 2` test-hooks image | existing VFS contract remains green; two-hart lifecycle markers and AP-13 are witnessed, with no VFS failures |

## Security and compatibility constraints

- The kernel is the sole authority for CellId/generation-to-root-TID mapping. Request bytes, `sender_tid`, and a raw CellId are never authority evidence.
- A VFS provider may resolve/watch only its current attested caller principal. This prevents Cell enumeration, cross-cell lifetime surveillance, and arbitrary death subscriptions.
- Principal-keyed state remains generation-sensitive and fail-closed; no “best effort” fallback from a missing owner record to CellId or sender TID is permitted.
- Root TID is a lifetime endpoint, not a capability to act as the root and not an authorization replacement for `(CellId,generation)`.
- Reuse begins only after full root-cell quiescence and owner-slot withdrawal. A successor cannot inherit old VFS state, kernel provenance, quota, or notification tokens.
- The new syscall IDs are append-only. Existing ABI numbers/layouts, IPC trailer length, message framing, service-TID masked receive behavior, CellId quota allocation, and Phase 07 ready-last atomic publication remain compatible.

## Closure record — 2026-08-22

**Closed:** `CELLOS-VFS-SMP-006_CLOSED_VERIFIED_RV64`.

**Verified evidence:** API `90/0`; RV32 release compilation; fresh test-hooks image; one-hart VFS `2/2`; and the dedicated RV64 two-hart VFS lifecycle regression `7/7`. The two-hart proof covers owner context, heartbeat retirement identity, quota-fault cleanup, root exit, retiring-syscall denial, lease install/revocation, owner watches, AP/init behavior, and VFS lifecycle cleanup. Final quality and security closure are PASS. The evidence reviews are [`agent://CrossArchClosureQuality`](agent://CrossArchClosureQuality) and [`agent://CrossArchClosureSecurity`](agent://CrossArchClosureSecurity); the fresh lifecycle execution record is [`agent://HeartbeatIdentityRetest`](agent://HeartbeatIdentityRetest).

**Revision reference:** `main` resolved to `85a5b873c5961c911ea8e04473c4fcb61de68b4a` during this status synchronization. This is a repository-reference hash, not a substitute for the execution evidence above.

**RV32 evidence boundary:** RV32 runtime was not executed because the host lacks OpenSBI firmware. This is a non-blocking host-firmware evidence gap; the RV32 release compile is recorded, and this plan does not claim RV32 runtime evidence.

**Unchanged gates:** Closing this VFS owner-lifetime ticket does not close full Phase 07, Phase 03 provenance/signature work, Phase 04 qualification, independent Tier 2 qualification, Phase 08, or any release/ledger/human-approval gate.
