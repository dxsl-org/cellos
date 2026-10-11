---
phase: 1
title: "Exact contract deltas and legacy-work adoption"
status: pending
priority: P1
tier: thinking
dependencies: []
---

# P01 — Contract, ABI and adoption gates

## Deliverable and requirements

Turn selected [Spec 25](../../docs/specs/25-cell-kernel-authority-contract.md) semantics and its detailed [Spec 26 workload extension](../../docs/specs/26-workload-placement-scaling-contract.md) into exact reviewed change sets. AC-26 and inventory/gates cover AC-01..AC-43; AC-01..AC-35 retain their requirements. Non-activating design/recon may proceed, but no public ABI changes or profile activation. Architecture approval is not approval of endpoint/manifest fields, ledger bytes or lifecycle protocol.

## Related files / ownership

- Inspect/amend through governance: `docs/specs/{15-kernel-boundary,17-ipc-wire-contract,20-unified-ipc-contract,22-native-domain-cell-implementation-gate,23-native-sdk-contract}.md` and the VFS directory-capability decision.
- Inventory: `libs/api/src/abi/`, `libs/api/src/services/`, `libs/types/`, `libs/ostd/`, `kernel/src/task/`, `kernel/src/cell/`, `kernel/src/loader/`, all service/runtime consumers.
- Create in this plan directory during execution: `abi-deltas.md`, `migration-ledger.md`, `ownership-handoffs.md`, `profile-budgets.md`. These are execution deliverables, not files claimed to exist now.
- Maintain `.agents/cell-contract-0024-locks.md`, portfolio and phase status together.

## Implementation steps

1. Read the lock register, reconcile each unfinished old slice against current source/evidence, and adopt necessary work into P02..P10. Do not require the locked plan itself to complete. Record exact former/new owner, paths, invariant and proof obligation.
2. Enumerate all exported symbols and consumers using LSP references where available (record fallback if server fails). Include tests, C headers, mlibc sysdeps, generation binding, direct kernel fd users and boot loader/VFS cycle. No inferred "only caller" claims.
3. Specify exact local handle representation, kernel identity encoding, lineage limit/accounting, shared CSpace locking, budget profile and operation/waitset ownership. Choose bounded values from measured current footprint; refuse overflow rather than allocate unboundedly. Define all error and output-buffer ownership paths.
4. Define CPU quota as aggregate execution time per replenishment period across all CPUs, a bounded burst and constrained priority; specify charging at context switches/ticks and bounded overshoot. Set exact parameters and validation tolerances before implementation, not after a passing benchmark.
5. Enumerate target semantic amendments: unhandled worker fault retires Cell; explicit post-start grants within fixed receive ceiling; POSIX optional linking; stream mechanism versus Unix adapter; no raw ring authority. Preserve same-incarnation explicit reply-token handoff (threads share trust).
   Inventory exact policy metadata for application/workload identities; allowed nodes/artifacts/templates/tiers/architectures/state locality; child subject/method/export/authority ceilings; replication-compatible schema and affinity map; node/workload/application budgets and per-replica charges; always-on router/controller envelopes. Scope one application in one admitted cluster, not federation/new trust domains. Every field needs exact encoding, units, finite bound, default/reject rule, version and publisher/parser/consumer owner; these are requirements, not preapproved ABI names/bytes.
   Inventory routing policy: logical versus concrete generation-pinned endpoint, original-subject proof, service schema, route membership/version, controller epoch/sequence, local monotonic publication/health receive age, bounded members/normalized weighted slots/locality bias, per-router/per-replica static concurrency shares and destination cap. Record admission/queue count/bytes/residence/deadline and one-selected-attempt/refusal semantics; signature validation is publication-time, not per-request local routing. Duplicate/lower sequences never renew freshness; share reallocation requires old-generation fencing and retained-work reconciliation, not router-restart counter reset or automatic borrowing.
   Inventory scaling telemetry/policy: one owning ingress per queued/in-flight request, source identity/epoch/sequence/window and receive-age units; targetConcurrencyPerReplica in requests, min/max, consecutive up/down windows, cooldown and sample durations, replica-per-window rate limits, pending-create caps, bounded coalesced cold-start wakes. Missing/stale coverage freezes decisions; worker counters cannot double count ingress demand.
   Inventory controller fixed-home identity/fence, application/reservation/create keys, durable schema and reserve/flush/spawn/reconcile/release, destination double-claim/key-reuse refusal, Ready/drain/retire and authoritative key closure. Record transaction/flush/storage limits, startup recovery, CPU common-period conversions and schema migration; timeout never releases unknown charge or promotes authority.
   `profile-budgets.md` lists finite Cell/replica/thread counts; aggregate CPU execution-time/period/burst; memory/capability metadata/queue bytes; capability/operation/completion slots; broker/session, lifecycle intent, telemetry/route-table and retained-outcome bounds. Record nested node/workload/router shares inside application budget, including router/controller overhead; reserve/release owner, crash/unknown/quarantine state, overflow outcome and positive-delta resize/pending shrink. Per-request admission consumes preallocated local shares, not durable global transactions.
6. Present each concrete public delta with compatibility/removal plan and exact revision/digest for Law-1 checkpoint 1. After its implementing phase supplies evidence, obtain checkpoint 2 on the actual delta before freezing/promoting. Do not record either checkpoint from architecture approval alone.
7. Map every AC-01..AC-43 to deterministic scenarios and runtime fixture owners. Inventory exact symbol → current/target owner → all callers/tests/docs/removal rows for concrete `RemoteEndpoint`, proposed logical endpoint, router proofs/counters, controller route/budget policy and supervisor/state-authority handoff. Proposed `cells/services/workload-router/` and `cells/services/placement-controller/` are not existing implementations. P08-A transport → C budget/placement → B routing/admission → D autoscale/reconcile → E failure qualification is the true internal gate chain, not five new phases; C must supply durable budgets, real ingress shares and bootstrap route membership before B admits live work, never mock grants. Schedule native/C++ worker-pool/node-B management fixtures, AC-35 actual concurrent clients and AC-43 measured multi-node balancing plus load-triggered automatic spawn/drain; request counts alone are insufficient.
8. Rebase protected KMS/Silo work with original Build/AC-012 gates; preserve relay-first then direct-LAN sequencing. Record any absent external prerequisite as a named gate, not a no-op implementation.

## Success criteria / verification

- [ ] Each changing public item has caller/test/doc migration rows; no double authority table or permanent shim.
- [ ] Every legacy unfinished slice has disposition; reciprocal blocks/blockedBy relations and direct-entry notices resolve to the controlling plan.
- [ ] Exact API/byte changes have checkpoint-1 records before any such edit; later phases explicitly await checkpoint 2 for release.
- [ ] Exact endpoint/manifest/lifecycle/budget/telemetry inventory covers original-subject routing authority, units/freshness, nested shares/control overhead, unknown creates, fixed-home fencing, bounded policy, Ready/drain and dynamic shrink/resize; each changing public/persistent symbol has approval and exhaustive migration rows.
- [ ] Run a document/graph verifier against lock register, phase links, acyclic dependencies, AC ownership and approval fields. Inspect its output; negative control deletes one link/edge in an in-memory copy and must be caught.
- [ ] Existing runtime evidence is labeled by source/artifact/profile, not rebranded as new-contract proof.

## Security and risk / rollback

No weakening of current CapSet, launch allowlists, Tier-2 deny, relay, signing or production posture. If an amendment is rejected, stop its affected slice and retain old enforcement; do not implement a parallel bypass. Documentation rollback preserves accepted history and a dated superseding decision, not deletion of evidence. No irreversible runtime state change in this phase.

## Assumptions and deviation log

Assumption: existing scope metadata may be stale; resolve by source/runtime evidence and explicit owner adoption, not unchecked checkboxes. Documentation amendment includes scoped workload routing/placement/autoscaling through AC-43 and the Spec 26 extension; it authorizes no implementation and preserves ten pending phases, all 38 locks and every independent gate.
