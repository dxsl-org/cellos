---
title: "Cell / Kernel Cell / authority contract cutover"
description: "Single execution owner for the Cell-centric Intel C2C cutover, including node-local execution, resource authority, recovery, placement/autoscaling, optional POSIX and distribution."
status: pending
priority: P1
tier: thinking
created: 2026-10-11
blockedBy: []
blocks:
  - .agents/260528-2016-vicell-full-implementation/plan.md
  - .agents/260603-1803-phase-h-vfs-hardening/plan.md
  - .agents/260605-1406-phase28-wasm-cells-epmp/plan.md
  - .agents/260605-1538-milestone-2-1-vfs-complete/plan.md
  - .agents/260605-2107-full-reliability-track/plan.md
  - .agents/260607-1200-phase-27-protocol-hardening/plan.md
  - .agents/260615-1325-vicell-app-sdk-v1/plan.md
  - .agents/260621-0830-cell-perms-p2-p5/plan.md
  - .agents/260624-0630-kernel-boundary-cleanup/plan.md
  - .agents/260707-1726-g2-loader-redesign/plan.md
  - .agents/260711-1917-tier3b-x86-vtx/plan.md
  - .agents/260712-0800-supervisory-cell-migration/plan.md
  - .agents/260712-0952-tier3b-vm-hardening-compat/plan.md
  - .agents/260712-1000-cell-package-distribution/plan.md
  - .agents/260712-1900-manifest-v2/plan.md
  - .agents/260712-1902-dice-attestation-identity/plan.md
  - .agents/260722-0917-g4-full-std-tier1/plan.md
  - .agents/260722-2330-tier3b-finish-g5-lite/plan.md
  - .agents/260727-2101-midori-lessons-cellos/plan.md
  - .agents/260801-midori-phase01-completion/plan.md
  - .agents/260801-parallel-midori-closure/plan.md
  - .agents/260808-1544-wx-cross-hart-tlb-shootdown/plan.md
  - .agents/260809-0922-sas-lbi-revocable-vfs-access/plan.md
  - .agents/260819-1416-port-common-drivers-g1-g2-g3/plan.md
  - .agents/260821-0642-app-tiers-completion/plan.md
  - .agents/260821-1700-tier1-baseline-admission/plan.md
  - .agents/260821-1800-tier1-rust-std-pal-feasibility/plan.md
  - .agents/260822-phase08-manifest-predesign/plan.md
  - .agents/260825-1726-kms-silo-production-root/plan.md
  - .agents/260825-sdk-delivery/plan.md
  - .agents/260826-1605-phase4-dev-reference-authority/plan.md
  - .agents/260827-1004-hardware-independent-roadmap/plan.md
  - .agents/260827-1015-sas-caller-owned-range-ledger/plan.md
  - .agents/260905-1139-sas-lbi-outcome-closure/plan.md
  - .agents/260927-0739-kernel-architecture-repair/plan.md
  - .agents/260927-1100-c2c-anywhere-tier-aware/plan.md
  - .agents/261004-1957-x86-pc-lane/plan.md
  - .agents/261006-0934-httpd-single-cell-concurrency/plan.md
lock: CELL-CONTRACT-0024
---

# Cell contract and authority implementation plan

**Planning deliverable:** architectural contract and detailed execution baseline selected by the maintainer on 2026-10-11. Implementation phases below are pending, not completed by writing this plan.
**Canonical acceptance:** [Spec 25](../../docs/specs/25-cell-kernel-authority-contract.md), AC-01..AC-43; AC-01..AC-35 retain IDs and requirements. **Detailed normative extension:** [Spec 26 workload placement/scaling](../../docs/specs/26-workload-placement-scaling-contract.md), no separate AC numbering.
**Decision and alternatives:** [ADR-0024](../../docs/decisions/0024-cell-kernel-authority-contract.md).
**Recon:** [scout report](scout-report.md). **Acceptance handoff:** [spec pointer](spec.md).
**Locks:** [CELL-CONTRACT-0024 register](../cell-contract-0024-locks.md); [portfolio](../plan-portfolio.md).

## Scope and entry

Implement the approved Cell-centric model end-to-end: Kernel Cell root/bootstrap, shared Cell object authority, safe node-local threads, bounded C2C operations, resource/namespace lifetime, recoverable lifecycle/state/evidence, optional POSIX, authenticated distributed calls and lifecycle, a scoped placement/autoscaling controller Cell with an application-wide budget, and explicit Intel VM guest adapters. ADR-0022 still sets Intel-only hardware scope. No new independent feature/platform program.

Next executable work is **P01 non-activating contract delta/adoption and evidence preparation**. No public ABI editing before its exact Law-1 checkpoint 1; no freeze/release before checkpoint 2. Maintainer direction approval is neither checkpoint for unspecified bytes. Spec 20 remains draft; ADR-0023's existing bytes remain frozen. Protected relay Build/activation, physical/purchase and production gates do not disappear.

One implementation slice at a time. Research can be independent; scheduler/CSpace/loader/syscall and shared SDK edits always have one integration owner. Each slice records exact file set, preconditions, acceptance IDs, and evidence ceiling before editing. Do not mass-enable Tier 2, grants, remote or snapshot to make an oracle pass.

One Cell belongs to one node; a node is one kernel instance on physical hardware or a VM, and a cluster is multiple nodes. A service Cell serves many concurrent requests through bounded operations/queues and workers, not one Cell per request. Kernel owns local execution/isolation/authority primitives only. Broker Cell owns peer sessions, reconnect, direct/relay selection and a bounded session/connection pool; service-net owns IP/TCP, and protected authority owns trust, identity and protected relay TLS. C2C hides transport selection, never failure semantics.

P08 includes explicit logical workload routing and replica placement/autoscaling for one application across selected nodes of one admitted cluster, not federation/new trust domains. A fixed-home fenced controller owns desired replicas, durable application budget and versioned membership; a workload-router Cell at each admitted ingress owns bounded selection/queues without a controller hop per request. Concrete endpoints remain generation-pinned; only replication-compatible typed methods opt into a logical pool, preserving kernel-attested original subject/exact method scope and destination authorization. Single-writer/shard/session affinity stays pinned; no DSM, resharding or writer cloning.

Router/controller envelopes and nested ingress/workload shares count toward application totals alongside reserved/live/draining/unknown children. No per-request durable budget transaction, silent share borrowing, hidden retry or ambiguous replay. Budget/key release requires terminal evidence fencing delayed publication; missing telemetry freezes scale decisions and controller outage closes mutations. Ready route validity never replaces destination live authority/state fencing. Local failure causes no implicit remote fallback. All 38 scoped locks, single-writer state, destination admission and relay-first/direct-LAN gates remain unchanged.

## Phases and dependencies

| Phase | Deliverable | Dependencies | Contract proof | State |
|---|---|---|---|---|
| P01 | [Exact contract/ABI deltas and adoption gates](phase-01-contract-gates.md) | none | AC-26; approval ledger and migration census | pending; documentation/evidence preparation admitted |
| P02 | [Kernel Cell, shared CSpace and bounded delegation](phase-02-kernel-authority.md) | P01 | AC-01..AC-05 | pending; exact authority ABI approval required |
| P03 | [Threads, SMP retirement and aggregate budgets](phase-03-thread-smp.md) | P02 | AC-06..AC-09; local AC-30 | pending |
| P04 | [Unified operation ownership and multi-wait completion](phase-04-operations.md) | P02, P03 | AC-10..AC-12; AC-06/09 | pending; wait/completion ABI gate |
| P05 | [Service resources, namespaces and device/buffer revoke](phase-05-resource-authority.md) | P02, P03, P04 | AC-02/04/05/13 | pending; service protocol gates |
| P06 | [Recoverable lifecycle, state and crash evidence](phase-06-recovery.md) | P04, P05 | AC-14..AC-18, AC-27, local AC-29 | pending |
| P07 | [Optional POSIX and native runtime cutover](phase-07-posix-runtime.md) | P04, P05 | AC-19 | pending |
| P08 | [Protected transport, workload routing, placement and scaling](phase-08-distributed.md) | P04, P05, P06; independent protected gates | AC-20..AC-23, AC-28/29; cluster AC-30..AC-34; AC-36..AC-42 | pending; remote off |
| P09 | [Intel VMX prerequisite and explicit guest adapters](phase-09-guest-adapter.md) | P04, P05, P07, P08 | AC-24 | pending; VMX qualification required |
| P10 | [Profile qualification and Linux comparison](phase-10-qualification.md) | P02..P09 | AC-25, AC-30..AC-35, AC-43 and complete ledger | pending; hardware/production rows separately gated |

P07 and P08 have independent prerequisites but WIP still serializes implementation. P09 may prepare VMX prerequisite design after P01; bridge implementation waits for listed dependencies. A local milestone may ship with P08/P09 explicitly disabled and pending; **the complete requested program is not done until all applicable phases pass**. Missing external evidence blocks only its named promotion, not local contract work.

P08 is internally gated in true order **A authenticated transport/lifecycle → C application budget/placement → B workload endpoint/routing/admission → D demand-driven autoscale/reconcile → E failure qualification**; milestone labels are not execution order. C supplies durable controller budgets, real ingress shares and versioned bootstrap membership before B can admit live routing; mock grants cannot substitute. Each milestone stays pending with its own symbol migration, positive/negative oracle and evidence gate; these are not new phases or new dependency edges. P10 then proves actual balanced multi-node load-driven automatic spawn/drain, not just client count.

## Ownership and clean cutover

| Boundary | Integration owner phase | Rule |
|---|---|---|
| `kernel/src/task/`, `kernel/src/cell/`, kernel loader | P02–P06 serialized owner | P03 consumes P02 identity; P04 consumes P03 lifetime; no parallel competing task/CSpace registries |
| `libs/api/src/abi`, `libs/types`, Spec 17 | P01 exact delta ledger; phase-specific implementation owner | Two Law-1 checkpoints per actual change; never repurpose a frozen opcode |
| `libs/ostd` SDK, consumers | P04/P05/P07 | Migrate every consumer of replaced semantics in the same cutover slice; remove obsolete implementation rather than permanent shims |
| VFS/NET/device state | P05/P06 | Issuer owns authoritative semantic table; kernel carries provenance, not duplicate ACL |
| KMS/Silo/service-net protected relay and broker | P08 adopts required work through named handoff | Protected authority owns trust/relay TLS; broker owns bounded peer sessions/reconnect/routes, service-net owns IP/TCP; ADR-0008/0009 and AC-001..012 remain binding |
| Workload router, controller, route publications and application budget ledger | P08-A→C→B→D→E; P01 exact SDK/manifest/protocol inventory | Proposed `cells/services/workload-router/` and `cells/services/placement-controller/`, not existing code; one integration owner per milestone migrates symbols/callers/tests/docs. Router owns original-subject bounded admission; controller owns fixed-home fenced policy/ledger and supplies real shares/membership before live routing; supervisor/issuer retain lifecycle/effect authority. |
| Guest/VMM and Intel physical qualification | P09/P10 | Adopt only relevant unfinished slices; AMD/QEMU does not qualify Intel VMX/physical |

Completed code is reused after source/artifact verification; checkbox metadata is not proof. No dependency requires executing a locked old plan. Unfinished prerequisite work is adopted under one new phase with its original safety criteria and named former-owner handoff. Historical kernel warm snapshot is not a prerequisite for service recovery; leave it closed rather than inventing proof.

## Approval and migration ledger requirements

P01 writes the exact affected-symbol inventory in this directory before coding: current → target owner, every caller/test/doc, preserved/removal decision, serialization/persistent state consequences, approval revision and result. It must cover:

- Reserved Kernel principal, shared CSpace handle/delegation/revoke API and all `CapSet`/path launch consumers.
- Whole-Cell unhandled-fault policy, aggregate CPU budget, thread-local runtime state.
- Existing bounded RPC IDs/statuses, multi-source wait/multi-waiter ownership and terminal reservation.
- VFS spawn-only authority versus bounded post-start receipt, explicit object transfer and namespace sealing.
- Lifecycle restore-ready-publication, crash record query, state schema/durable request outcomes.
- POSIX optional linkage, legacy fd/stream consumers and BootFS boot cycle.
- Concrete `RemoteEndpoint` and proposed logical endpoint, original-subject proof, typed method replication eligibility/schema/affinity, route membership/version/controller epoch/sequence and local monotonic freshness; exact SDK/service/schema publisher/parser and every caller/test/doc owner.
- Allowed nodes/artifacts/templates/tiers/architecture/state-locality/child-authority ceilings; finite node/workload/application/replica and router/controller envelopes; preallocated routing concurrency shares, bounded WRR normalized slot/member limits and optional bounded locality weight. Units include bytes, counts/slots and CPU execution-time entitlement in a common replenishment period, not hardware speed.
- Telemetry owner/source epoch/sequence/window/receive age, authorized ingress queued+in-flight request counts without worker double count; targetConcurrencyPerReplica (requests), min/max, consecutive up/down windows, cooldown/sample/age durations, replica-per-window rates, pending creates, queue count/bytes/deadline and bounded coalesced min-zero wake.
- Lifecycle create/reservation keys, fixed-home fence, reserve-before-spawn, destination double-claim/key-reuse refusal, unknown reconcile and key closure against delayed publication; restore/fence/Ready routing, safe drain, nested share reclaim, positive-delta resize and below-use pending shrink. Every changing public/persistent surface needs its own exact approval and migration row.
- Remote incarnation/export/lifecycle and Intel guest-bridge surfaces retain explicit owner/caller inventories; no local-pointer authority promotion or weakening of existing guest/remote gates.

Feature flags may isolate unqualified test images but must not create dual authority truths or a permanent compatibility alias. Frozen public identifiers stay reserved where removal rules require it; preserving a reserved slot is not retaining an executable obsolete path.

## Verification protocol

Tests establish invariants; a separate actual runtime scenario establishes the changed path. Each phase records source revision, artifact SHA-256, features/configuration, runner command, full serial logs, negative controls, expected versus observed outcome, and ceiling (host / x86 QEMU / exact physical Intel / production). Every persistent result remains pending until observed. A skipped test or absent runner never counts as pass.

Existing reusable commands (read from repository, **not run for this planning change**):

- `bash scripts/build-x86_64-c2c-lifecycle-ci.sh` then `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test local-service-lifecycle-x86`.
- Domain/SMP build and runtime: `scripts/build-x86_64-domain-test-ci.sh` and `scripts/x86/qemu-domain-test.sh`; P03 verifies supported arguments and uses `X86_CPUS=2` with positive second-CPU execution markers, not just a QEMU flag.
- `scripts/qemu-c-pthread.sh`, `scripts/qemu-futex-test.sh`, `scripts/qemu-tls-test.sh` are existing runtime precedents; current architecture/feature coverage must be inspected before using them for an Intel claim.
- `scripts/run-c2c-broker-oracle-qemu.sh` is a local RV64 broker precedent, not an Intel/two-node qualification runner.
- P08 creates an isolated two-node runner and behavioral oracle; no existing command or remote PASS is assumed.

Before optimizing, measure a representative typed service workload: one worker, multiple threads/Cells, crash/rebind and remote request. P10 AC-35 runs actual hundreds of simultaneous clients locally and across admitted replicas, preserving request identity/deadlines/effect/uncertainty; request count is not Cell count or throughput. AC-43 separately observes sustained authorized demand → bounded controller decision → reserved automatic spawn on another node → Ready publication → measured weighted/capped multi-node routing, then safe load-driven drain. Include missing telemetry/partition, cold-start expiry, dropped/delayed create, drain-route races and dynamic shrink. Compare same-hardware Linux DAC and explicit namespace/capability/LSM-hardened configurations at matched semantics/durability; report latency distribution, throughput, CPU, memory/queue/authority bytes, per-replica selections/occupancy, lifecycle/ledger transitions and correctness. No universal Linux-superiority claim.

## Risks and rollback

High risks: shared authority races; unhandled worker fault corrupting shared memory; completion waiter loss; generation reuse; revoke versus hardware in flight; acknowledgement before durable commit; partitioned stale writer/controller; double allocation or early release of uncertain-create budget; unsafe drain/state relocation; ambient POSIX dependencies; feature flags accidentally bypassing admission.

Each phase has explicit rollback and irreversible boundary. Prefer known-safe image + cold boot with unsafe/unqualified profiles closed. Never resurrect old authority tokens or replay state solely to make a downgrade boot. After external effects or persistence format/epoch changes, restore/forward-migrate under policy; binary revert cannot undo them. Kernel and driver faults outside containment remain node failures.

## Assumptions

- Existing artifacts may be stale versus source. Verify image hashes and runner coverage before reuse; the previous review's QEMU PASS is baseline evidence only.
- Exact protected persistence/time/identity and Intel physical devices are not qualified by this plan. P08/P10 must retrieve real gate evidence; missing evidence leaves profiles closed.
- Tables, CPU budget periods and byte quotas need workload-sized profile values; P01 freezes bounded parameters and P10 measures them. No unbounded default or hidden allocation allowed.
- The full language/runtime/Tier-3 feature set is not implied by a C++ or guest smoke. Preserve each profile's explicit restrictions.

## Red Team Review and validation

[Validation and review record](validation.md) owns adversarial findings, decisions and document consistency output. Every AC in Spec 25 maps to a phase above. Implementation completion requires that ledger, affected callers/tests/docs, smoke evidence and removal of obsolete paths; documentation completion does not unlock old plans.

Integration-owner adversarial review closed five concrete gaps: final replacement watermark,
cross-node deadline limits, crash-safe keyed creation, completion-consumption eligibility and
state-authority failover fencing. AC-27..AC-29 append those observable cases without renumbering.
The cluster amendment appended AC-30..AC-35 for admission, budgets, placement/scaling/drain and concurrent workloads. The detailed workload amendment appends AC-36..AC-43 for endpoint/routing authority, bounded weighted selection/queues, deterministic placement, nested budgets, sampled scaling, outage/drain safety and actual load-driven multi-node evidence. No runtime proof is added.
Specialist reviewer jobs returned provider errors, not independent passes. Prior document smoke
verified 161 classified legacy plans, 38 reciprocal scoped locks, an acyclic 10-phase DAG
and 29 acceptance owners, with three rejecting negative controls; this is historical evidence,
not validation of the amended 43-criterion documents. See the record for exact scope.

Deviation log: none at plan creation. During implementation append decision, reason, affected contract and revert path immediately. Contract-breaking or irreversible changes need a new approval, not a silently narrowed acceptance criterion.
