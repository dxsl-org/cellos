---
phase: 3
title: "State-Required Service Replacement"
status: scope-gated
priority: P1
effort: ""
dependencies: []
tier: thinking
---

# Phase 03: State-Required Service Replacement

> Log Decision / Deviation / Surprise immediately. Preserve atomic cutover and exact authority; no new hotswap ABI or cold-success mode.

## Overview
Close M1 hot-swap semantics: the existing state-preserving hotswap operation must not report success after missing or invalid state. Cold start remains ordinary restart, not an indistinguishable hotswap success.

## Requirements
- Use existing SnapshotTimeout/ReadyTimeout/error replies. Do not add public flags, enum values, syscall or manifest bytes.
- Snapshot/restore failure aborts and preserves the old live provider when rollback is still possible.
- Ready is emitted only after successful state validation/application by a trusted replacement.
- Preserve mailbox capacity preflight, FIFO commit, capability ceiling, cached-old-TID rejection and supervisor authorization.
- Initiator authorization and hotswap stash isolation must be kernel-instance-bound, not task-name or caller-chosen-key conventions. This is capability enforcement inside trusted SAS, not a malicious-native-code sandbox claim.

## Architecture
Current supervisor accepts SnapshotTimeout (`hotswap.rs:113-120`); both demo Restore handlers call sys_hotswap_ready after an error. Fixing timeout alone is insufficient.
The kernel owns transport cutover, while the service owns schema/state validity. The supervisor trusts the authorized service's ready acknowledgement; this is not proof against a malicious Tier-1 service.

## Assumptions
- SOURCE-RESOLVED: existing replies cannot encode separate invalid-schema or rollback-failure reasons. Keep wire values unchanged: code 6 means replacement did not become ready within the bound, not a schema diagnosis. Preserve underlying failure code; correlate replacement-owned restore diagnostics with supervisor-owned rollback steps, published provider and cleanup results by transaction/instance in test evidence. Do not invent supervisor knowledge without a message. CLI must not infer successful rollback from an error reply. No extra enum/opcode is authorized.
- REQUIRED DESIGN CHECK: before Build, specify a bounded snapshot-completion/cancellation fence and service quiescence protocol using existing interfaces. It must prevent post-snapshot acknowledged mutations and late stash writes after rollback clear. Current pause/poll/clear alone cannot prove either. If current interfaces cannot express the invariant, keep this slice scope-gated for an exact design delta; other evidence lanes continue.
- SOURCE-RESOLVED AUTHORITY GATE: supervisor trusts a display name, while SpawnFromMem can produce that name; the stash is a global key->bytes map. Before Build, specify an unforgeable authorized launch principal and source-generation/swap/replacement binding for hotswap state. Use current trusted launch/reservation interfaces only if they can prove these bindings; otherwise obtain the exact design delta. A predictable key, first named sender or valid schema is not authorization.

### Design-gate result

Current interfaces cannot prove the required authority and cancellation invariants:

- `PauseService(service_id, expected_tid)` authenticates the supervisor and hides one exact TID, but records neither the initiating supervisor instance nor a swap ID/source generation.
- `StateStash`, `StateRestore`, and `StateStashClear` accept caller-chosen non-argv keys. Any admitted holder can overwrite, read, or clear the same hotswap key; the kernel stores no source/replacement/transaction owner.
- `SpawnReplacement(old_tid, path)` binds replacement authority to the frozen source, but that binding does not extend to a particular stash key.
- Supervisor request admission uses the sender's display name. `SpawnFromMem` can reproduce that name, and the available process query exposes no unforgeable launch principal.
- Freezing changes scheduler state but supplies no completion/cancellation token for an already-delivered Snapshot handler or in-flight stash write. Pause/poll/clear therefore cannot exclude a late publication after rollback cleanup.

The minimal safe delta needs an explicit kernel-owned hotswap transaction bound to `(supervisor instance, source CellId/generation/root TID, service, swap ID, replacement TID)` plus a kernel-attested requester principal and atomic cancel/clear fence. Source stash, supervisor probe/clear, replacement restore/ready, commit, and rollback must each authorize against that record. The old service must also quiesce after publishing its envelope until commit or a cancellation it can observe. Encoding these meanings by overloading a zero-length restore or a predictable key would be a second implicit protocol and is rejected.

That delta requires new internal/public authorization surface not approved by this phase's no-new-ABI constraint. Build remains scope-gated; Phase 04's independent generic rows may proceed.

## Related Files
- Modify: `cells/services/supervisor/src/hotswap.rs`, current error/transfer/main handlers only as necessary without wire changes.
- Modify: `cells/demos/hotswap-demo-v1/src/main.rs`, `cells/demos/hotswap-demo-v2/src/main.rs` and other actual ViStateTransfer participants discovered before editing.
- Modify: `cells/tests/bench/src/scenarios/hotswap_supervisor.rs`, `cells/tests/bench/src/scenarios/hotswap_cli_probe.rs`, `tests/integration/tests/hotswap-smoke.rs`.
- Modify: `cells/tools/sys-tools/src/bin/hotswap.rs` diagnostics as required, preserving numeric status meanings; kernel-local fence changes need an explicit phase-local design and Main-owned shared-file boundary.
- Conditional kernel-local ownership/fence changes: `kernel/src/cell/state_stash.rs`, `kernel/src/task/syscall.rs`, existing task/launch/replacement metadata and service reservation owner. Main serializes overlap with Phase02. Preserve atomic commit in `kernel/src/cell/hotswap.rs`, `kernel/src/cell/service_registry.rs` and all legitimate argv/non-hotswap stash flows discovered through references.
- Modify: `docs/hotswap-guide.md`, relevant `docs/system-architecture.md`/reliability claims and changelog via Main.

## Implementation Steps
1. Inventory Snapshot/Restore/ready, generic stash and launch-principal consumers. Freeze source owner-generation, transaction, replacement and authorized probe/clear roles before coding. Bind hotswap stash insert/overwrite/read/clear to those roles; unrelated holders of syscall allowlists cannot substitute or delete its state. Reproduce missing-stash/restore-error, forged-name requester and foreign overwrite/clear cases. No blanket generic-stash rewrite or new ABI is pre-approved.
2. Make SnapshotTimeout fatal; do not merely clear once and resume. Fence completion/cancellation of the already-delivered snapshot request, check rollback operations, then restore old-provider discovery and finally reclaim transaction stash. Inject delayed publication after timeout and prove it cannot recreate an unreachable stash; an unproven fence blocks this outcome rather than passing the tiny fast demo.
3. Make actual replacement participants signal ready only after successful version/length validation and state application. On invalid state refuse readiness so the existing bounded wait returns ReadyTimeout; distinguish diagnostics in structured evidence. Never cold-start then send success. Failed rollback/cleanup is recorded independently, not hidden behind the initiating error.
4. Keep supported empty application state valid when it has the existing valid serialization envelope; distinguish it from absent/truncated stash. Do not define generic schema layout in the kernel.
5. Exercise the snapshot-to-hard-freeze interval with cached-old-TID input. If acknowledged mutations are lost, use existing service quiescence/freeze mechanisms to stop mutation and preserve queued input after snapshot publication. If the current protocol cannot express safe resume/rollback, record the exact contract gap and obtain a separate design delta; no claim of closure by simply avoiding the interleaving.
6. Preserve pre-commit rollback and atomic commit. A post-commit cleanup failure cannot be reported as an old-provider rollback success; record actual published generation/outcome. Do not duplicate external side effects by replaying ambiguous requests.
7. Treat the existing 500-yield loops as iteration bounds, not a measured five-second deadline. Preserve their behavior unless real bounded-wait correctness requires a separately documented internal monotonic deadline; no unrelated retry redesign.
8. Extend consumer-observable regression scenarios: counter readback, provider identity, retained authority, old-TID refusal and queued request outcome. Missing QEMU/image prerequisites are explicit failure/BLOCKED in acceptance invocation, not silent test success.
9. Update guide to the real soft-pause -> snapshot -> hard-freeze -> restore/ready -> commit protocol and distinguish stateful upgrade from ordinary cold restart.

## Success Criteria
- [ ] Missing/failed stash and invalid restore never publish a successful cold replacement.
- [ ] On pre-commit failure with live old provider, same provider remains reachable with all acknowledged state; replacement/stash/reservations are reclaimed, including delayed snapshot completion. No successful rollback claim if any required rollback step failed.
- [ ] Valid empty state and valid nonempty state both restore under the service's existing envelope/schema.
- [ ] Cached-TID requests around snapshot/freeze/commit have no silently lost acknowledged mutation; accepted FIFO and post-cutover old-TID rejection remain covered.
- [ ] Successful CLI and supervisor scenarios preserve the independently observed counter, not only startup/ready text.
- [ ] Status code 6 retains readiness-bound meaning; invalid-state and rollback diagnostics are truthful separate evidence, with published identity/readback independently checked.
- [ ] Unauthorized swap remains denied; no public ABI or replacement capability widening; no exactly-once claim for arbitrary external side effects.
- [ ] An admitted sender with forged `hotswap` display name cannot initiate cutover; foreign clear/overwrite/read attempts cannot affect the active swap. Legitimate CLI, source, supervisor probe, replacement, argv and non-hotswap flows retain their intended authority.

## Security Considerations
Replace name-only initiation checks with exact kernel-authenticated launch/instance authority; revalidate generation across the transaction. Hostile cases exercise existing capability boundaries, not arbitrary trusted-native memory corruption. Test injection must not weaken normal stash admission. Preserve prior source-bound approvals as historical records; shared-kernel edits require fresh affected evidence, not automatic reuse of an old manifest or promotion.

## Risk Assessment
Revert as a matched supervisor/demo change and rebuild image. Before commit rollback must preserve provider/state; after commit use a new governed replacement rather than pretending old publication never happened. Already executed application side effects are not reversible by a source revert. No external deployment authorized.

## Deviation Log
- Red-team F1/F2 accepted: added the late-completion fence and exact status/rollback evidence boundary. No unsupported new wire error is implied; Phase03 closure depends on proving the named fence.
- Red-team T1/T2 accepted: current name-only requester authentication and unowned stash contradict the stated authorization/state-preservation outcomes. Added exact-principal/transaction design gates, owned source boundaries and hostile acceptance cases. T3 is covered by F2's explicit generic-failure/independent-rollback evidence decision.
- Source investigation confirmed both mandatory design gates. The current syscall set cannot bind a caller-chosen stash key to the paused source generation/replacement transaction, cannot attest the CLI launch principal to the supervisor, and cannot fence late snapshot publication on cancellation.
- Decision: do not implement only the visible timeout/cold-ready fixes. They would leave foreign overwrite/clear, forged-name initiation, and post-timeout stash recreation possible.
- Required approval: a bounded kernel transaction/principal interface with atomic cancel/clear semantics, followed by matched supervisor, participant, hostile-probe, integration, and documentation changes.
