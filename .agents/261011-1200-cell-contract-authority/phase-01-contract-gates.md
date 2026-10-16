---
phase: 1
title: "Exact contract deltas and legacy-work adoption"
status: complete
priority: P1
tier: thinking
dependencies: []
---

# P01 — Contract, ABI and adoption gates

## Deliverable and requirements

Turn Spec 25 v1.3 and Spec 26 v1.1 into exact reviewed change sets. AC-26 and inventory/gates cover AC-01..AC-50, including explicitly approved amendments to earlier semantics. Non-activating design/recon may proceed; no public ABI changes or profile activation. Architecture approval does not approve endpoint/manifest fields, ledger bytes or lifecycle protocol.

## Related files / ownership

- Inspect/amend through governance: `docs/specs/{15-kernel-boundary,17-ipc-wire-contract,20-unified-ipc-contract,22-native-domain-cell-implementation-gate,23-native-sdk-contract}.md` and the VFS directory-capability decision.
- Inventory: `libs/api/src/abi/`, `libs/api/src/services/`, `libs/types/`, `libs/ostd/`, `kernel/src/task/`, `kernel/src/cell/`, `kernel/src/loader/`, all service/runtime consumers.
- Maintain preparation artifacts in this plan directory: `abi-deltas.md`, `migration-ledger.md`, `ownership-handoffs.md`, `profile-budgets.md` and [measurement launcher contract](measurement-launcher-contract.md). They exist as preparation records; existence does not complete P01 or approve exact deltas.
- Per-family handoff: [23 preparation packages](exact-deltas/index.md) with independent document/source byte pins. Their revisions are not exact CP1 candidate approvals; resolve package-specific blocking inputs before assigning exact candidate revision/digest in `abi-deltas.md`.
- Dependency-cycle resolution: [checkpoint amendment](dependency-checkpoint-amendment.md), superseded solely for the named slices by [reviewed user-requested authorization](scoped-g1-g2-authorization.md) and [pinned manifest](scoped-g1-g2-authorization.json). This admits controlling P01 G1→G2-R-core/B edits, not legacy execution or23downstream approvals.
- Source-bound reviewed candidates: [G0 reconciliation](g0-instrumentation-reconciliation.md), [G1r2 Intel accounting](g1-local-accounting-candidate.md), [G2r3 persistence](g2-ordinary-persistence-candidate.md). G2-J blocked; actual-diff/caller/lock review, event bounds and disposable/private capture restrictions still gate runtime evidence.
- Implemented software handoff: [G1actual review](g1-implementation-review.md), [18G1captures](../../build/g1-final-runtime-summary.json), [G2actual review](../../build/g2-runtime/implementation-review.json), [G2functional/fault/VM evidence](../../build/g2-runtime/runtime-summary.json). Both authorized development slices implemented/exercised; no production profile, G2-J or P01 completion.
- Current prerequisite reconciliation: [measured profile](measured-profile-candidate.md) and [issuer/persistence](issuer-persistence-candidate.md) distinguish implemented G1/G2 software evidence from still-unavailable finite profile, immutable journal, admitted backend/floor and issuer decisions. Historical baseline refusals remain historical; neither implemented development slice completes an entire prerequisite family.
- G2-J design preparation: [preservation/recovery design](g2-j-preservation-design.md) and [finite-layout evidence](g2-j-finite-layout-evidence.md) specify the separately gated journal slice. Source geometry and bounded model evidence do not authorize persistent byte changes or certify physical durability, global trace capacity or production budgets.
- Maintain `.agents/cell-contract-0024-locks.md`, portfolio and phase status together.

## Implementation steps

1. Read the lock register, reconcile each unfinished old slice against current source/evidence, and adopt necessary work into P02..P10. Do not require the locked plan itself to complete. Record exact former/new owner, paths, invariant and proof obligation.
2. Enumerate all exported symbols and consumers using LSP references where available (record fallback if server fails). Include tests, C headers, mlibc sysdeps, generation binding, direct kernel fd users and boot loader/VFS cycle. No inferred "only caller" claims.
3. Specify exact local handle representation, kernel identity encoding, lineage limit/accounting, shared CSpace locking, budget profile and operation/waitset ownership. Choose bounded values from measured current footprint; refuse overflow rather than allocate unboundedly. Define all error and output-buffer ownership paths.
4. Define CPU quota as aggregate execution time per replenishment period across all CPUs, a bounded burst and constrained priority; specify charging at context switches/ticks and bounded overshoot. Set exact parameters and validation tolerances before implementation, not after a passing benchmark.
5. Enumerate target semantic amendments: unhandled worker fault retires Cell; explicit post-start grants within fixed receive ceiling; POSIX optional linking; stream mechanism versus Unix adapter; no raw ring authority. Preserve same-incarnation explicit reply-token handoff (threads share trust).
   Inventory exact policy metadata for application/workload identities; allowed nodes/artifacts/templates/tiers/architectures/state locality; child subject/method/export/authority ceilings; replication-compatible schema and affinity map; node/workload/application budgets and per-replica charges; always-on router/controller envelopes. Scope one application in one admitted cluster, not federation/new trust domains. Every field needs exact encoding, units, finite bound, default/reject rule, version and publisher/parser/consumer owner; these are requirements, not preapproved ABI names/bytes.
   Inventory routing policy: concrete/logical endpoints, immutable origin proof and origin-issuer single-target record, Destination Ready provenance, route version/controller epoch/sequence and receiver-start challenge fields/lifetimes/source sets; bounded weighted slots, static shares, queues/deadlines and no-retry semantics. Publication proof checks remain off the local request path.
   Inventory telemetry: required ingress-generation sets, authoritative old-operation reconciliation and retired-share fences; challenge-window identity and eligible homogeneous domain/resource/template/concurrency class; min/max, demand/T, hysteresis/cooldown/rates/pending/wakes. Missing coverage freezes both directions; no worker double count or new-zero replacement.
   Inventory independent Authority fixed-home principal/fence, issuer-precommitted genesis/control envelopes and one-use bootstrap grant; durable reserve/live destination claim/keyed recovery/closure schemas and every issuer/supervisor/parser/storage caller. Controller policy uses Authority client only; no controller ledger minting. Exact persistence/encoding/flush limits require approval; timeout cannot reclaim unknown charge or promote authority.
   `profile-budgets.md` lists finite Cell/replica/thread counts; aggregate CPU execution-time/period/burst; memory/capability metadata/queue bytes; capability/operation/completion slots; broker/session, lifecycle intent, telemetry/route-table and retained-outcome bounds. Record nested node/workload/router shares inside application budget, including router/controller overhead; reserve/release owner, crash/unknown/quarantine state, overflow outcome and positive-delta resize/pending shrink. Per-request admission consumes preallocated local shares, not durable global transactions.
   For baseline workload measurements, use the existing init-owned configured autoload route per [launcher contract](measurement-launcher-contract.md), with isolated config/artifact provenance and bounded capture. No new fixture feature or shell authority is required by the selected design. Existing async probes are pinnedCPU0: record this limitation on twoCPU runs and reserve aggregate CPU/SMP proof for actual distinct-CPU workloads and approved charging semantics.
   Apply [resource attribution contract](resource-attribution-contract.md) before choosing finite budgets: generation-attested publication/retirement records; distinct Cell charge versus physical frames/kernel heap metadata; no per-thread double count, counter-reset reclamation or latency-clock conversion of scheduler ticks. Fixed parked counts1/4/8 and zero-child controls are proposed measurement workloads, not existing runtime proof. Private bounded instrumentation must be reviewed before code and cannot loosen owner-watch access.
   Follow [instrumentation owner review](instrumentation-owner-review.md): subject/attempt/route identity must survive internal handoffs, caller Elf path is only a hint, host capture boot labels are not kernel-attested freshness, and TID-only/deferred release records cannot prove generation attribution. Keep actual heap metric global until historical owner metadata exists. Prove capture arm/capacity and exact allocator-lock peak insertion; include guard frames/retention before provenance loss. Source review is not approval of an unshown implementation.
   Use [private trace specification](private-trace-spec.md) for the selected x86 root-only measurement schema:160byte records, conditional maximum219events in256slots, fixed counter/epoch/obligation storage and explicit unknown/overflow rejection. Host-run labels are not attestation; snapshot groups are not simultaneous multi-lock state. Review actual producer/closing gate, source epochs and obligations before implementation/runtime acceptance; the throwaway model does not complete this phase.
   Apply [producer/close protocol](producer-close-protocol.md): native atomic STOP+active gate, one strong CAS attempt per admission/reservation, ready Release/Acquire and balanced guard release. Contention invalidates observation rather than spinning under owner locks; counter-only hooks also enter the gate. Finite interleaving model is design evidence only; actual lock/memory safety, hooks and final validity export require implementation verification.
   Execute later preparation only within [source-bound implementation entry](instrumentation-implementation-entry.md): explicit BootstrapInit pre-ready binding, logical attempt context distinct from short observation guard, immutable subject identity and atomic/source-owned auxiliary fields. Proposed private kernel build gate excludes storage from normal images and is not yet implemented. No configured-entry public token, observer authority or ABI migration is authorized; real diff/coverage review precedes resource interpretation.
6. Present each concrete public delta with compatibility/removal plan and exact revision/digest for Law-1 checkpoint 1. After its implementing phase supplies evidence, obtain checkpoint 2 on the actual delta before freezing/promoting. Do not record either checkpoint from architecture approval alone.
7. Map all AC-01..AC-50 to deterministic scenarios and runtime owners. Inventory existing endpoints and proposed `cells/services/{budget-authority,placement-controller,workload-router}/` with every caller/test/doc/removal row. P08-C owns issuer-precommitted genesis, Authority ledger/fence and live claims before B routing. Inventory challenge proofs/source sets, old-generation fences, homogeneous domains, immutable maxima/current allocations, Destination Ready evidence and origin-local issuer-sealed one-attempt proof. Exact ABI/persistence approvals precede implementation.
8. Rebase protected KMS/Silo work with original Build/AC-012 gates; preserve relay-first then direct-LAN sequencing. Record any absent external prerequisite as a named gate, not a no-op implementation.

## Success criteria / verification

- [ ] Each changing public item has caller/test/doc migration rows; no double authority table or permanent shim.
- [ ] Every legacy unfinished slice has disposition; reciprocal blocks/blockedBy relations and direct-entry notices resolve to the controlling plan.
- [ ] Exact API/byte changes have checkpoint-1 records before any such edit; later phases explicitly await checkpoint 2 for release.
- [ ] Exact endpoint/manifest/lifecycle/budget/telemetry inventory covers original-subject routing authority, units/freshness, nested shares/control overhead, unknown creates, fixed-home fencing, bounded policy, Ready/drain and dynamic shrink/resize; each changing public/persistent symbol has approval and exhaustive migration rows.
- [ ] Run a document/graph verifier against lock register, phase links, acyclic dependencies, AC ownership and approval fields. Inspect its output; negative control deletes one link/edge in an in-memory copy and must be caught.
- [ ] Existing runtime evidence is labeled by source/artifact/profile, not rebranded as new-contract proof.
- [ ] Baseline workload launcher uses reviewed init→bench→probe edges and exact private config/image provenance; denied launch, absent START/terminal, timer conversion gaps and unobserved reclamation are not promoted to success. Existing lifecycle scenario does not stand in for cancellation, multithread or distributed acceptance.
- [ ] Resource evidence joins exact incarnations, reports live/residual/retained partitions and trace overflow, and does not call polling maxima event-complete high-water. Quarantine/deferred/unknown obligations prevent settled qualification; existing process/heap counters are not production budget proof.

### Current closure disposition — G1/G2 continuation

The checkboxes above represent complete criteria, not document existence or scoped software passes. Keep them unchecked until their whole obligation is met.

| Criterion | Current disposition / next exact obligation |
|---|---|
| 1 — caller migration | Lexical source/package inventory exists; candidate-specific semantic/generated/provider/external reference closure and every symbol's cutover remain required. G1/G2 private/software implementation does not close public migration. |
| 2 — legacy adoption | All38 adopted ownership rows and reciprocal locks are recorded. The document verifier exercises graph/direct-entry consistency; ownership proof gates remain with implementing phases and no legacy execution is unlocked. |
| 3 — exact checkpoints | The narrow G1/G2 development grant is recorded separately. All23 downstream delta CP1/CP2 records remain pending; no approval is inferred from successful software runs. |
| 4 — exact bounded contracts | G1 supplies source-bound private accounting; G2 supplies ordinary open/error/provisioning semantics. Complete issuer-bound public/persistent encodings, finite full vectors and journal/Unknown/fence recovery remain unresolved. |
| 5 — verifier | Execute current consistency and approval-refusal modes with actual in-memory negative controls after preparation changes; retain separate receipts in validation. Consistency success is not approval. |
| 6 — historical labels | Original failed and passing matrices, source/artifact/profile identities and lane limitations remain separate. G1 is Intel-vendor QEMU-TCG software evidence; G2 is portable RV64 VM evidence, not device durability. |
| 7 — launcher/provenance | Existing reviewed init launch edges and bounded raw captures supply scoped software evidence. Unobserved timer, cancellation, multithread lifecycle or distributed scenarios are not inherited from these runs. |
| 8 — resource qualification | G1 proves its bounded same-generation occupied-time/terminal replay and distinct-CPU overlap. It does not certify global resource settlement, all-path capacity, production enforcement or a selected finite budget. G2 does not retain Unknown charges or preserve acknowledged journal checkpoints across crash. |

## Security and risk / rollback

No weakening of current CapSet, launch allowlists, Tier-2 deny, relay, signing or production posture. If an amendment is rejected, stop its affected slice and retain old enforcement; do not implement a parallel bypass. Documentation rollback preserves accepted history and a dated superseding decision, not deletion of evidence. No irreversible runtime state change in this phase.

## Assumptions and deviation log

Existing scope metadata may be stale; resolve through source/runtime evidence and named owner adoption. The approved five-group red-team amendment extends the contract through AC-50, preserves ten pending phases and 38 locks, and waives no independent gate.
## Proposed N02-A prerequisite handoff — not active

The user selected preparation of [native development handoff r1](native-development-handoff-r1.md), not its implementation. Controlling P01 owns the proposed exact cspace.rs/actual-module harness invariant tranche; current phase criteria and CP1/CP2 remain pending. The lock register, portfolio and P02 record the same conditional disposition. Only an explicit later decision naming the reviewed packet revision/digest activates it; WIP is one tranche and no successor opens automatically. Exact allowed paths/output records and host/native evidence ceilings are in the packet. Public/persistent/SDK changes and missing D05/D07, issuer/backend/physical gates remain blocked.

