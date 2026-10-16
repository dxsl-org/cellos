---
phase: 6
title: "Transactional lifecycle, committed state and crash evidence"
status: pending
priority: P1
tier: thinking
dependencies: [4, 5]
---

# P06 — Recoverable services, not merely new processes

## Deliverable and requirements

AC-14..AC-18, AC-27 and local foundations of AC-29. One prepare/restore/ready/publish lifecycle for spawn, crash replacement and planned hotswap; independently owned committed state; bounded crash evidence and client recovery. Distinguish worker crash from node/power loss durability.

## Related files / ownership

- `cells/tools/init/src/{service_table.rs,supervisor.rs}`, `cells/services/supervisor/src/hotswap.rs` and control handlers.
- `kernel/src/cell/{hotswap.rs,state_stash.rs,service_registry.rs}`, `kernel/src/task/{launch.rs,scheduler.rs}`, `kernel/src/audit.rs`.
- `cells/services/{config,vfs}/src/`, `cells/tools/shell/src/{shell_state.rs,state_transfer.rs}`, SDK state-transfer/client contracts and the native stateful benchmark.
- Existing `libs/cellos-fs` persistence facilities and VFS service; diagnostics query/tool consumers located in P01. Create service-side state journal/diagnostic reader modules in their existing owning subsystem, not a second kernel filesystem.

## Implementation steps

1. Converge init restart and supervisor replacement on a lifecycle transaction with authority, exact artifact, desired namespace/resource ceiling, state revision and target logical binding. Policy remains outside kernel; atomic binding/retirement stays kernel-enforced.
2. Prepare replacement unpublished. Stop old writer admission, drain/classify outstanding mutations, atomically fence and capture the final committed revision, then restore/catch up to that watermark and reacquire new issuer-granted handles. Publish only after readiness. Before fencing, failure may retain the old live service; after fencing, it stays unavailable unless the authority issues a fresh writer epoch. Never publish a stale snapshot or reuse an invalidated epoch.
3. Select one bounded stateful service workload with increment/read and request-result reconciliation. Put its state authority outside the execution worker; support declared RAM-worker-crash and durable-node-reboot classes using existing VFS/CellosFS, not whole-kernel snapshot.
4. Define versioned journal/checkpoint records, commit marker/flush order, checksum and request-result atomicity; bounded retention/compaction must not turn an expired request into automatic replay. Schema migration is explicit; reject old/poison/incomplete state. No new database/ORM is required; retain native storage facilities.
5. Fence worker writes with monotonic service ownership epoch checked at the state/effect authority; replacement cannot commit while the old writer is unfenced. External nontransactional effects get explicit unknown/reconcile behavior, not a fake exactly-once result.
6. Update config/shell/VFS transfer contracts and actual handlers consistently. Application session fields are explicitly persistent or volatile; VFS/NET open handles are reacquired, never restored as live credentials. State stash becomes scoped/owned/versioned with bounded allocation or is retired for migrated consumers; no anonymous overwrite/truncation as success.
7. Capture bounded kernel fault record before reclaim, with valid artifact/PC/incarnation fields and explicit unknown operation/revision fields. Diagnostics Cell persists/queries/symbolicates outside trap context; it is restartable and cannot block termination. Retention/drops observable.
8. Make clients retain their Cell incarnation while provider returns unavailable/recovering; explicit rebind/reopen obtains newly granted resources. One-for-one by default; any cascade is a named recovery-domain policy. Bound retries only before admission; accepted uncertain writes require reconciliation.
9. Add stable creation-key registration to kernel publication under the approved lifecycle delta. The lifecycle authority writes durable intent, publishes/reconciles the keyed child, then records result; coordinator restart must query the existing publication before creating anything. A node reboot marks old-boot incarnations gone; original create replay is not automatic restart authorization. P08 exposes this primitive through scoped remote management.

## Verification / success criteria

- [ ] Inject failure at prepare, initialize, restore, reacquire and publish: no partial ready binding, no widened rights, reservations released/quarantined, deterministic recovery outcome.
- [ ] Real worker crash before commit, after commit before reply, and during restore; acknowledged mutations survive, duplicate request identity yields same committed result, old worker cannot commit.
- [ ] Commit concurrently with replacement preparation; final published state includes every acknowledged pre-fence mutation. Inject failure after fence and prove old token cannot resume committing.
- [ ] Crash lifecycle coordinator after intent, after publication and before durable result; same-boot recovery finds one keyed child. After node reboot, replay reports prior incarnation gone and does not create another child without explicit restart policy.
- [ ] Durable profile save→cold reboot→recover with torn/partial write injection and stale/poison/schema mismatch negatives. RAM profile explicitly loses node-reboot state rather than claiming durability.
- [ ] Shell/config state selected for persistence survives their crash; deliberately volatile state is documented. VFS and NET client resources reopen or return explicit unrecoverable session outcome while client Cell remains unchanged.
- [ ] Crash records survive worker reclaim; offline symbolication resolves a known injected PC for exact artifact. Flood beyond capacity yields correct dropped count; unavailable Diagnostics Cell does not hang crash handling.
- [ ] Repeated replacement cycles show no increasing retained operations, caps, mappings, quota or state-stash slots after quiescence.

Correction 2026-10-16: prior checkmarks and cited standalone-model/storage-boot receipts did not establish these criteria. Acceptance remains pending; historical observations are not runtime qualification.

## Security and rollback

Do not restore serialized authority. Required state missing/invalid is a failed recovery, never a permission bypass. Durable data/epoch/schema changes are irreversible by binary revert: use tested compatible backup/forward migration, deny older format/epoch and record lost external effects. Warm snapshot qualification remains closed and is not silently promoted.

## Assumptions and deviation log

Existing serializers are not evidence of wired crash restore. Existing filesystem persistence requires exact flush/reboot evidence for this journal; historical filesystem tests alone do not prove the new acknowledgement rule. Deviation log: none.
