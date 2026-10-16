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
**Canonical acceptance:** [Spec 25](../../docs/specs/25-cell-kernel-authority-contract.md), v1.3, AC-01..AC-50. The approved red-team checkpoint preserves IDs but amends affected earlier semantics. **Detailed normative extension:** [Spec 26 workload placement/scaling](../../docs/specs/26-workload-placement-scaling-contract.md), v1.1, no separate AC numbering.
**Decision and alternatives:** [ADR-0024](../../docs/decisions/0024-cell-kernel-authority-contract.md).
**Recon:** [scout report](scout-report.md). **Acceptance handoff:** [spec pointer](spec.md).
**Locks:** [CELL-CONTRACT-0024 register](../cell-contract-0024-locks.md); [portfolio](../plan-portfolio.md).

## Scope and entry

Implement the approved Cell-centric model end-to-end: Kernel Cell root/bootstrap, shared Cell object authority, safe node-local threads, bounded C2C operations, resource/namespace lifetime, recoverable lifecycle/state/evidence, optional POSIX, authenticated distributed calls and lifecycle, a scoped placement/autoscaling controller Cell with an application-wide budget, and explicit Intel VM guest adapters. ADR-0022 still sets Intel-only hardware scope. No new independent feature/platform program.

Scoped G1Intel-private and G2-R-core/B development implementation/software evidence are delivered below under the [reviewed authorization](scoped-g1-g2-authorization.md). The original manifest baseline remains historical; explicit source continuations preserve it. Remaining prerequisite blockers require exact journal preservation/finite-layout review and external backend/floor/issuer/profile inputs, not further implicit source permission. G2-J and downstream ABI/CP1/CP2/production/remote remain closed; Spec20/ADR-0023 and protected/hardware/purchase gates are independent.

One implementation slice at a time. Research can be independent; scheduler/CSpace/loader/syscall and shared SDK edits always have one integration owner. Each slice records exact file set, preconditions, acceptance IDs, and evidence ceiling before editing. Do not mass-enable Tier 2, grants, remote or snapshot to make an oracle pass.

One Cell belongs to one node; a node is one kernel instance on physical hardware or a VM, and a cluster is multiple nodes. A service Cell serves many concurrent requests through bounded operations/queues and workers, not one Cell per request. Kernel owns local execution/isolation/authority primitives only. Broker Cell owns peer sessions, reconnect, direct/relay selection and a bounded session/connection pool; service-net owns IP/TCP, and protected authority owns trust, identity and protected relay TLS. C2C hides transport selection, never failure semantics.

P08 includes logical workload routing and replica placement/autoscaling across selected admitted cluster nodes. The independent fixed-home **Budget Authority Cell** owns durable application-budget enforcement, issuer-precommitted genesis and live destination reservation claims. The controller owns policy, desired replicas and versioned membership, never ledger minting. Routers own bounded selection/queues without controller or Authority transactions per request. Concrete endpoints remain generation-pinned; logical pools require issuer-verified Destination Ready and immutable origin-operation proofs with one issuer-sealed target. Affine state stays pinned.

Authority/controller/router envelopes and ingress/workload shares consume the same finite application vector. Challenge rounds bound route/health/capacity/telemetry freshness from receiver-local round start, not arrival. Unresolved old ingress generations freeze both scale directions until fenced reconciliation; automatic demand/T applies only within homogeneous interchangeable domains. Immutable incarnation maxima bound mutable admitted allocations. Unknown children remain charged; release closes delayed publication. All 38 locks and existing protected/ABI/hardware gates remain unchanged.

## Phases and dependencies

| Phase | Deliverable | Dependencies | Contract proof | State |
|---|---|---|---|---|
| P01 | [Exact contract/ABI deltas and adoption gates](phase-01-contract-gates.md) | none | AC-26; approval ledger and migration census | pending; documentation/evidence preparation admitted |
| P02 | [Kernel Cell, shared CSpace and bounded delegation](phase-02-kernel-authority.md) | P01 | AC-01..AC-05 | pending; exact authority ABI approval required |
| P03 | [Threads, SMP retirement and aggregate budgets](phase-03-thread-smp.md) | P02 | AC-06..AC-09; local AC-30; shared AC-48 | pending |
| P04 | [Unified operation ownership and multi-wait completion](phase-04-operations.md) | P02, P03 | AC-10..AC-12; AC-06/09 | pending; wait/completion ABI gate |
| P05 | [Service resources, namespaces and device/buffer revoke](phase-05-resource-authority.md) | P02, P03, P04 | AC-02/04/05/13 | pending; service protocol gates |
| P06 | [Recoverable lifecycle, state and crash evidence](phase-06-recovery.md) | P04, P05 | AC-14..AC-18, AC-27, local AC-29 | pending |
| P07 | [Optional POSIX and native runtime cutover](phase-07-posix-runtime.md) | P04, P05 | AC-19 | pending |
| P08 | [Protected transport, workload routing, placement and scaling](phase-08-distributed.md) | P04, P05, P06; independent protected gates | AC-20..AC-23, AC-28/29; AC-30..AC-34; AC-36..AC-42; AC-44..AC-50, shared AC-48 | pending; remote off |
| P09 | [Intel VMX prerequisite and explicit guest adapters](phase-09-guest-adapter.md) | P04, P05, P07, P08 | AC-24 | pending; VMX qualification required |
| P10 | [Profile qualification and Linux comparison](phase-10-qualification.md) | P02..P09 | AC-25, AC-35/43 and qualification of all AC-01..AC-50 | pending; hardware/production rows separately gated |

P07 and P08 have independent prerequisites but WIP still serializes implementation. P09 may prepare VMX prerequisite design after P01; bridge implementation waits for listed dependencies. A local milestone may ship with P08/P09 explicitly disabled and pending; **the complete requested program is not done until all applicable phases pass**. Missing external evidence blocks only its named promotion, not local contract work.

P08 execution remains **A transport → C Budget Authority genesis/claims and controller placement → B router admission → D homogeneous-domain autoscaling → E failure qualification**. C supplies enforced shares and destination-backed membership before live routing; fixture grants cannot substitute. Ten phases and dependency edges remain unchanged.

## Ownership and clean cutover

| Boundary | Integration owner phase | Rule |
|---|---|---|
| `kernel/src/task/`, `kernel/src/cell/`, kernel loader | P02–P06 serialized owner | P03 consumes P02 identity; P04 consumes P03 lifetime; no parallel competing task/CSpace registries |
| `libs/api/src/abi`, `libs/types`, Spec 17 | P01 exact delta ledger; phase-specific implementation owner | Two Law-1 checkpoints per actual change; never repurpose a frozen opcode |
| `libs/ostd` SDK, consumers | P04/P05/P07 | Migrate every consumer of replaced semantics in the same cutover slice; remove obsolete implementation rather than permanent shims |
| VFS/NET/device state | P05/P06 | Issuer owns authoritative semantic table; kernel carries provenance, not duplicate ACL |
| KMS/Silo/service-net protected relay and broker | P08 adopts required work through named handoff | Protected authority owns trust/relay TLS; broker owns bounded peer sessions/reconnect/routes, service-net owns IP/TCP; ADR-0008/0009 and AC-001..012 remain binding |
| Workload router, controller and independent Budget Authority | P08-A→C→B→D→E; P01 exact SDK/manifest/protocol inventory | Proposed `cells/services/{workload-router,placement-controller,budget-authority}/`, not existing code. Authority enforces durable budget/genesis/live claims; controller owns policy/membership; router verifies Ready and origin-local one-attempt attenuation. Supervisor/issuer retain lifecycle/effect authority. |
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
- Concrete and logical endpoints, authenticated Destination Ready provenance, immutable origin payload/deadline/operation proof and issuer-sealed single target; exact publisher/parser/SDK callers and removal inventory.
- Allowed placement nodes/artifacts/tiers/locality and immutable maxima versus current allocation; charged Authority/controller/router envelopes and enforced finite ingress shares.
- Receiver-start challenge rounds, required ingress-generation sets, fenced old-demand reconciliation and homogeneous scaling domains; bounded telemetry, queues, hysteresis, cooldown and pending work.
- Budget issuer precommitted genesis, one-use supervisor bootstrap key, independent fixed-home Authority ledger/fence, destination live atomic claim, unknown recovery, terminal key closure, delta growth and pending shrink. Every public/persistent change requires its own approved migration row.
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

[Current red-team report](red-team-2026-10-11.md) records four completed independent lenses, 14 raw findings merged into eight gaps and five accepted Major groups (zero Critical). Maintainer selected independent Budget Authority enforcement. Spec 25 v1.3 and Spec 26 v1.1 amend affected earlier criteria and append AC-44..AC-50.

Mitigations: bounded challenge freshness; old-generation demand reconciliation and homogeneous scaling domains; independent budget/genesis/live claims and immutable maxima; destination Ready provenance; immutable origin request with issuer-sealed single-target attenuation. No rejected or deferred findings. [Validation record](validation.md) retains historical failed review attempts separately; interrupted editing jobs are not review passes.

Document consistency checks cover acceptance ownership, links, ten-phase DAG and reciprocal locks. These are not runtime, ABI, protected-service or hardware qualification. P01 non-activating preparation remains next; 38 locks stay active.

Deviation log: none at plan creation. During implementation append decision, reason, affected contract and revert path immediately. Contract-breaking or irreversible changes need a new approval, not a silently narrowed acceptance criterion.

## Validation Log

### 2026-10-12 — Post-red-team planning validation

Questions asked: **0 new**. No materially different unresolved product choice was found in the ten phases. Previously explicit maintainer decisions are reused, not presented as answers obtained in this session: architecture before code, independent Budget Authority, application-wide finite budget and bounded automatic placement/scaling. Fixed-home/no-promotion and affinity/no-replay boundaries remain the selected contract; no approval is inferred for exact ABI bytes or activation.

### Verification Results

Tier: **red-team evidence reuse / marker-only**, per validate workflow skip rule. New implementation claims checked: **0 | Verified: 0 | Failed: 0 | Unverified: 0 marker claims**. Read all ten phases and overview; no `[UNVERIFIED]` or `[STALE]` markers require source resampling. These zero counts do not certify future implementation: earlier scout/source claims retain their original evidence ceiling, and all 50 runtime acceptance criteria remain pending. Current document smoke is recorded in [validation.md](validation.md).

### Decisions and phase propagation

- Preserve architecture-only scope and ten-phase DAG; no cook/code execution, unlocked legacy plan or ABI checkpoint is implied by “continue”.
- Clarify shared AC-48 ownership: P03 proves local immutable maximum/current-limit primitives; P08-C proves real Authority grant/claim growth and replacement accounting; P10 qualifies both. P03 cannot depend on P08 or enable fixture-backed application growth. This clarifies the existing shared criterion, not a scope reduction or rejected architecture.
- No newly rejected approach; failure-history unchanged. Independent post-amendment security review remains unperformed.

### Unqualified gates and handoff

| Gate | Owner / required observable evidence | What remains closed |
|---|---|---|
| Exact authority/endpoint/manifest/persistent bytes | P01 migration census and Law-1 checkpoint 1 on exact revision; checkpoint 2 after phase evidence | Public ABI edits before approval; freeze/release before checkpoint 2 |
| Finite profile parameters | P01 measured baseline footprints and explicit units, finite bounds and rejection semantics; P10 workload measurements | Unbounded defaults and capacity/throughput promises |
| Durable genesis/fence/claim and committed-state recovery | P06/P08 actual crash/flush/reboot/keyed reconciliation oracles, not historical filesystem success | Application-managed growth before real Authority integration; empty ledger recovery |
| Protected identity/time/relay | P08 protected evidence and independent Build/activation gates; relay-first then separately qualified direct LAN | Remote activation or plaintext/direct bypass |
| Intel physical/VMX and production | P09/P10 exact machine/artifact qualification plus independent approvals | Physical/VMX/production claims from host or QEMU-only evidence |

Next handoff: `/home/dmin/cellos/.agents/261011-1200-cell-contract-authority/plan.md`. `$hc-cook` may execute only the named [scoped G1/G2 grant](scoped-g1-g2-authorization.md), one slice at a time, with its candidate/path/source pins and actual-diff/pre-capture gates. Auto execution cannot widen it to G2-J, later public changes or activation. All38legacy locks remain active.

## P01 non-activating preparation — 2026-10-12

Preparation artifacts: [ABI delta inventory](abi-deltas.md), [legacy migration ledger](migration-ledger.md), [ownership handoffs](ownership-handoffs.md), [profile budgets](profile-budgets.md). These inventories expose outstanding measurements, caller closure and exact approval prerequisites; they do not complete P01 or authorize implementation.

Source quota constants are not application/cluster limits or measured profile capacity. The current non-experiment quota identity array has 64 entries with zero excluded by non-kernel reservation; current operation storage is per Task, and the watchdog is not aggregate CPU quota. Exact new profile values remain pending measurement/approval.

Canonical cleanup removed two obsolete predecessor paragraphs from Spec 25 that still assigned budget reservations to the controller. The approved independent Budget Authority contract is unchanged; no acceptance ID, phase dependency or lock is removed.

All ten phases remain pending and all 38 scoped locks active. Public/persistent bytes require exact Law-1 checkpoint 1 before edits and checkpoint 2 before release; missing physical/protected/persistence evidence keeps the affected profile closed.

Observed preparation smoke: 22 documents, 264 local links, zero missing paths; 38 scoped adoption rows and reciprocal locks, 50 acceptance-owner rows, ten pending phases and acyclic dependencies. In-memory cyclic-edge, missing-dependency and missing-link controls were rejected. Link anchors and exhaustive source references were not certified. Configured rust-analyzer exited on a reference request; ABI inventory explicitly records the fallback and incomplete caller closure.

### Census and baseline continuation

Consumer preparation is split into [local authority/operation census](consumer-census-local.md) and [compatibility/generated/bootstrap census](consumer-census-compat.md), complementing the ABI family inventory. They describe inspected in-repository consumers and remaining closure obligations; no out-of-tree or binary consumer completeness is inferred.

[Measurement readiness](measurement-readiness.md) records an actual `free` interaction on the existing hashed x86 development ISO under QEMU-TCG 8.2.2, with network disabled and snapshot mode. It supplies one aggregate committed-frame snapshot, not current-source provenance, marginal Cell cost, high-water, aggregate CPU quota, physical Intel/VMX or new-contract qualification. Exact finite profiles stay unapproved.

Current VMX root-entry source was located at `hal/arch/x86/src/x86_64/vmx.rs`; the historical absent VMCS filename does not mean all Intel virtualization source is absent. Full backend/source/runtime closure remains P09; this TCG baseline reports SVM and supplies no VMX witness. No locked predecessor is released.

Continuation findings: NET socket IDs are issuer-local despite legacy “kernel-issued” comments; VFS/NET owner-watch failure policy differs; shared completion queue still has one waiter/unfiltered drain; captured same-Cell reply-token handoff already exists and must survive cutover. Compatibility census exposes automatic POSIX selection, separate mlibc suppression/linkage, provider-specific `stat` declarations and missing immutable mlibc pin. These are migration/closure obligations, not newly authorized repairs.

Observed continuation smoke: 25 documents / 276 local links / zero missing paths; ten pending phases / acyclic DAG / 38 reciprocal locks and adoption rows / 50 AC owner rows; three in-memory negative controls rejected. Raw shell allocator row passes total/used/free arithmetic and the booted ISO digest matches measurement readiness. Source-to-image provenance, semantic reference completeness, finite new profiles and all new-contract runtime criteria remain unproved.

### Pinned current-kernel measurement continuation

Built current kernel source with locked/offline dependencies, release x86 target and development defaults + test-hooks into isolated outputs; assembled and booted a new ISO without changing source or overwriting the earlier image. [Provenance manifest](pinned-x86-provenance.json) records 798 local inputs stable before/after, compiler/configuration, commands and kernel/ISO digests. Reused embedded init/ramdisk are hashed binary inputs; their service-source rebuild and exhaustive external/toolchain provenance remain open.

[Measurement readiness](measurement-readiness.md#current-kernel-pinned-continuation--2026-10-12) and [pinned serial evidence](evidence-2026-10-12-pinned-x86.log) retain two actual shell allocator snapshots and eight boot-stack watermark rows. This supplies a pinned kernel baseline, not marginal/high-water capacity, aggregate CPU quota or safe new parameters. The run also reports `X86-VMM-SMOKE: FAIL e1=Preempted e2=Preempted`; no overall selftest/virtualization PASS is claimed, no failure was suppressed or repaired, and physical/VMX/protected/production qualification remains closed.

P01 and all subsequent phases stay pending; 38 locks and both exact ABI checkpoints remain unchanged. Next measurement prerequisites are source-pinned service packaging and complete incremental/retained/high-water accounting; later runtime changes still require their approved exact contracts.

Pinned preparation smoke: 25 documents / 281 local links / zero missing paths; acyclic ten-phase pending DAG and 38 retained scoped locks; three negative controls rejected. All 798 recorded input digests remained consistent, ISO digest matched, two allocator rows satisfied arithmetic, eight watermark rows fit allocated/usable bounds, and the known VMM failure remained in serial evidence. These checks do not certify exhaustive build provenance or new-contract acceptance.

### Source-rebuilt selected-service measurement

Rebuilt and dev-signed11 executable Cells through existing F1/F5; ten ramdisk executables passed independent byte readback, signed init is separately embedded, and policy/init/ramdisk/kernel/ISO digests plus exact command chain are retained in [service provenance](service-x86-provenance.json). The3174 recorded local input hashes matched observed snapshots; capture timing and non-exhaustive external/toolchain scope are explicit. Full init/ramdisk→kernel→ISO byte containment also passed. Initial serial selection was refused (library-only driver-serial); actual driver-serial-x86 was rebuilt without weakening signing.

[Readiness record](measurement-readiness.md#source-rebuilt-service-image--2026-10-12) and [serial evidence](evidence-2026-10-12-service-x86.log) now supply baseline for the source-rebuilt selected image, rather than reusing service binaries. Both shell snapshots and eight boot-stack watermarks were observed; no marginal/high-water/CPU/SMP/control-envelope budget or exhaustive current-application image claim. NET build has four warnings; VMM smoke failure remains, so no overall selftest/virtualization PASS. Current baseline reports development Tier2 ENABLED on oneCPU, distinct from the older historical disabled image; this opens no new-contract/remote/production gate.

The selected service packaging evidence gap is addressed. All phases,50new-contract ACs,38scoped locks and exact Law-1 checkpoints remain pending/unchanged; remaining P01 work is finite measured parameters, complete candidate-specific caller/artifact closure and exact contract proposals, not another prerequisite run of a locked old plan.

Selected-service preparation smoke:25documents/286local links/zero missing paths, tenpending phases/acyclic DAG,38retained locks/adoption rows,50ACowner rows and three rejected negative controls.3174recorded input hashes,11signed Cells,10ramdisk executable readbacks, twoallocator snapshots/eightstack rows and final embedding/ISO digest checks are preserved. Known VMM failure is still explicit; no new-contract qualification or exact-parameter approval is implied.

### Two-CPU measurement launcher closure

Same ISO booted with two CPUs; existing handoff/shootdown/topology/SAS/pin-dying development markers were observed. Attempted shell→bench async-lifecycle was denied before scenario START: no workload completion, reclamation, throughput or CPU-quota evidence. [Readiness](measurement-readiness.md#two-cpu-launch-readiness-scenario--2026-10-12), [runtime log](evidence-2026-10-12-service-x86-smp.log) and [scenario provenance](service-x86-smp-provenance.json) record the exact result.

The initial proposal for a new init-owned opt-in fixture was superseded after current configured-launch source reconciliation: [launcher contract](measurement-launcher-contract.md) selects existing autoload.toml structured argv and reviewed init→bench→probe edges. No new feature, normal shell authority change or policy widening. Earlier denial/log remains unchanged. Current work remains planning/evidence only; all phases, ACs, locks and ABI checkpoints are unchanged.

Launcher contract defines one scenario per isolated image, restart=never,150second host observation watchdog, exact bounded-leg evidence and a separate resource/CPU/placement measurement matrix. Probe pinningCPU0 and existing maximum-of-eight latency labeling prevent SMP/production-p99 overclaims. Existing host configuration tests passed18tests with explicit Linux target after the initial bare-metal target failed to supply the test crate; the proposed config still needs actual image/runtime validation.

### Configured lifecycle baseline executed

Rebuilt/signed four executables and isolated minimal config image without source/ABI/policy widening. Existing init→bench→probe autoload route now runs on oneCPU/twoCPU: both bounded-eight/reverse-correlation, parked/busy TrySend and fourPeerGone death-leg counters satisfy the existing scenario and emit PASS. [Readiness/result](measurement-readiness.md#configured-bounded-lifecycle-runtime--2026-10-12) and [provenance](configured-async-provenance.json) supersede earlier config NOT_RUN status only for this image. No explicit bench incarnation/TID publication or settled resource accounting; probes still pinnedCPU0. Strict launcher qualification, aggregate CPU/distributed ACs, P01 and all38locks remain unchanged.

### Resource attribution design

[Generation/resource contract](resource-attribution-contract.md) defines development-only identity trace, fixed parked counts1/4/8 plus zero-child controls, exact live/residual/retained partitions and settled/event-complete high-water requirements. Existing ProcessInfoV2 repeats shared Cell charge per task; owned_bytes is not disjoint physical ownership and cpu_run_ticks is tick sampling. Owner-watch access is not a generic observer API. No service registration/permission widening or arbitrary leak tolerance. Proposed trace/workload remains unimplemented; numeric production profile limits and exact checkpoints remain pending.

### Scoped instrumentation review

Two independent publication/resource reviewers produced eleven findings, adjudicated into five Major gaps and one Minor accounting correction in [owner review](instrumentation-owner-review.md). Resource contract now pins subject/attempt/route metadata propagation, host-only boot capture labeling, post-removal obligation identity, global actual-heap scope, allocator-lock high-water, retention/guard accounting and unproven256event capacity. No claim that existing measurement_log is lossless. Mitigations are planning requirements, not implemented fixes or full-plan red-team certification; no phase/lock/checkpoint changes.

### Private trace design model

[Private x86 trace spec](private-trace-spec.md) fixes160byte records, subject/attempt/source propagation, capture gate requirements, component/obligation counters and snapshot epochs. Compact schema bounds selected N≤8root-only runs to51+21N≤219records, leaving37of256slots; full serialized fixed payload56896bytes plus separately measured control/alignment/export overhead. [Model evidence](private-trace-model-evidence.json) checks offsets/capacity, eight invalid-record/presence/duplicate-final cases and five settled-negative cases. This is mathematical/serialization design evidence, not implemented tracing, lock-free/concurrency proof or resource runtime acceptance. Normal ABI/profile/source and all phase/lock/checkpoint states unchanged.

### Bounded producer/close protocol modeled

[Protocol](producer-close-protocol.md) selects single-attempt strong CAS admission/slot reservation, native STOP+active observer count, release-ready publication, balanced guard release and safe-point close/drain. Counter-only observation is included; contention/late work invalidates capture without changing runtime semantics. [Finite model](producer-close-model-evidence.json) checks20774strong-atomic interleavings over normal/one-slot/counter-only/faulted-writer cases. No producer spin/new lock or false complete state on the modeled overflow/fault paths. Model does not prove Rust/weak-memory correctness, hook coverage or actual resource evidence; all phase/lock/checkpoint states remain pending/unchanged.

### Source-bound instrumentation handoff

[Implementation entry](instrumentation-implementation-entry.md) maps actual request/state/return/reaper ownership and separates logical attempt lifetime from short active observation guards. Source review corrects configured-ingress arm to explicit real BootstrapInit owner/digest binding before ready push; no new public token or init feature. Private kernel build opt-in, absent/changed compound metrics and atomic/source-owned auxiliary access are explicit proposed requirements, not existing code. [Decision model](implementation-entry-model-evidence.json) checks16build-selection and five source-custody cases; not actual build/kernel verification. This preparation handoff defines blast radius and rollback while preserving10pending phases/38locks/independent ABI checkpoints.

### Private implementation runtime evidence — 2026-10-12

Private instrumentation now builds and executes; this supersedes earlier statements that the preparation has no source implementation, not the architecture/ABI gates. `build/p01-private-final/build.json` binds the isolated artifacts. `build/p01-private-final-captures/matrix.json` retains three fresh one-CPU boots each for 0/1/4/8 children and denied control through VM termination.

All twelve normal boots have structurally complete traces, zero final/late invalid flags and lifecycle outcomes with requested=admitted=exited. Record counts are 49/70/133/217 respectively. None qualifies as fully settled resource evidence: source-unavailable/incoherent metrics and unknown component accounting remain explicit, not zero. Consequently two-CPU qualification is gated off; no production budgets or throughput claim follows.

Denied control fails honestly: the loader refuses `/bin/vfs`, but userspace observes `Err(Unknown)`, not the fixture's expected PermissionDenied; the workload exits 1 and capture flags=32. No public error mapping or authority behavior was changed to make the fixture pass. Preserve all three invalid captures.

Observer contention/full/unready/late each ran three fresh boots; all twelve fault captures are rejected. Evidence: `build/p01-final-fault-{contention,full,unready,late}-captures/matrix.json`. Parser tests pass 14/14, including post-final workload rejection. A fresh `CELLOS_P01_TRACE=0` build at `build/p01-final-disabled/x86_64-unknown-none/release/cellos-kernel` contains none of P01-RECORD/FINAL/SNAPSHOT/MANIFEST; the historical directory named private-disabled had subsequently been reused for enabled smoke builds and is not omission evidence.

Collector finalization now requires STOP followed by Acquire drain and an opaque Frozen proof; no pre-drain resource finalization. Parser requires workload and immutable sidecars before final status, preserves source denial semantics, and binds image/QEMU operands to recorded digests. Public ABI, signing, authority, profiles, ten pending phases and 38 locks remain unchanged. No commit. Preparation runtime verification is recorded, but P01 resource qualification and denied-control acceptance remain unmet.


### Source-coherent counters and denied control — superseding runtime evidence

`build/p01-resource-coherent/build.json` binds the updated private kernel/fixture. Final reviewed-parser evidence is `build/p01-resource-reviewed-captures/matrix.json`: three fresh boots each at 0/1/4/8 and denied, all fifteen structurally complete. All three denied controls independently pass `denied_control_accepted`; none is a normal resource-qualified workload. Public wrappers remain unchanged: syscall namespace1/error4 PermissionDenied encodes -1, and the existing spawn decoder exposes Err(Unknown). The parser requires both exact source refusal and that public observation, exact parent incarnation/Pinned route, no child publication, and unique workload-before-closing order. Target path is fixture/build-bound, not present in the raw result record.

Source-owned versioned counters now cover quota charge, allocated stacks including guards, segment bytes, admitted live subjects, nonterminal operations, reserved completion slots, outstanding cleanup and unknown accounting obligations. Immutable object metadata retains subject ownership even if optional trace token reservation fails. Counter and epoch overflow fail closed; object terminal and actual destruction remain separate. Independent review found no actionable source-counter or lifecycle ownership regression. Two parser findings were corrected: duplicate JSON keys and incorrect known metric source-owner tags are rejected. Parser tests pass24/24; integrated enabled build and fresh disabled build succeed, with private export tags absent from the disabled binary.

One-CPU accounting qualification remains false in all twelve normal boots. Metric13 physical retained union is still absent, and class8 historical object ownership is still unknown; child settled-stage metrics remain unknown rather than fabricated success. Zero-child control also exposes raw heap/quota/frame residuals and outstanding obligations; no drift subtraction or runtime reclamation fix was applied.

Exact missing prerequisites: a bounded incarnation-owned retained physical-range ledger at pin Full, deferred coalescing/abandonment, forgotten stack and page-table/quarantine edges with overlap reconciliation; and historical allocation-owner metadata across allocation/transfer/delayed free. Current byte-only release tokens and execution-context quota charges cannot prove either. These require separately reviewed ownership instrumentation, not a guessed sum or public ABI/error remap. Existing invalid captures remain retained. Two-CPU qualification, production budgets, ten architecture phases and38locks stay gated; no commit.


### Ownership extension boundary — source investigation

Source investigation confirms allocator-only attribution is insufficient: `heap.rs:136-188` charges the executing Cell at allocation and refunds the executing Cell at free, with no pointer-owner record; `task/launch.rs:254-274` binds the selected subject only after preparation allocations already exist. A historical ledger therefore needs pre-publication attempt ownership, explicit adoption at publication and preserved allocation origin through delayed free. Recording current Cell alone would misattribute child preparation/kernel bookkeeping. Do not change production refund behavior as part of measurement.

Physical provenance exists transiently at `pin.rs:342-379` (base/pages/release key), `deferred_release.rs:117-166` (actual OwnedFrame vector before coalescing or quarantine), and stack/page-table retention edges. A bounded private sidecar can preserve these facts before loss and reconcile range overlap, but fixed subject count9 does not bound fragmented retained frame entries. Capacity must be derived from selected artifact/page-table inventory and retain overflow invalidation, never guessed from child count.

Two genuinely different next scopes: (a) a disposable fixed-capacity diagnostic ledger, with explicit attempt adoption/range joins and incomplete-on-overflow, leaving production metadata/quota untouched; (b) persistent allocation/frame ownership as a runtime model, changing object metadata and lifecycle contracts and requiring a separately reviewed architecture phase. The second is not authorized by the existing private P01 scope. Numeric production budgets, resource qualification and class8 checked-zero remain unapproved until the selected design is implemented and observed.


### Private ownership ledgers implemented and smoke-observed

User selected disposable private ledgers, not production ownership. Added heap origin ledger (1024 observation entries, selected preparation attempt adoption, existing heap-lock serialization, historical origin retained across realloc/free) and retained physical-range ledger (2048 observation entries, overlap reconciliation, range tokens preserved across deferred/quarantine transfers and actual release). Observation ceilings are not proven fit bounds. Allocator quota/refund and reclamation policy remain unchanged.

Integrated build `build/p01-ownership-ledgers-corrected/build.json` succeeds for0/1/4/8/denied; parser24/24 tests pass. Actual one-child QEMU smoke completed the lifecycle and exported70records, final/late flags0, heap origin invalid0 with fit/coverage0, and coherent retained source epoch22 with zero live ranges. Host stopped the VM after25seconds (exit124 expected from timeout); this is smoke evidence, not the150second sealed qualification matrix. Raw output is retained in session artifact230. Class8 remains unknown and settled metrics remain unknown; parser explicitly blocks qualification when heap capacity/coverage is unproven.

Remaining limits: argv/inherited objects and shared preparation bookkeeping prevent complete semantic allocation ownership; selected prepublication domain retention without binding faults rather than becoming zero. Range export permits independent end-state union inspection, not historical per-stage range replay. No claim of allocator/realloc fault injection, ledger saturation coverage, twoCPU validation or new full qualification matrix. No ABI/profile/authority/signing change, lock release or commit.


### P01 closure checks and ownership corrections

Private origin export now carries raw allocations and immutable attempt bindings; retained ranges preserve original and current reasons, and stale tokens cannot retire recycled slots. Exclusive argv, task/name/cwd/driver-list and CellSegments backing allocations are adopted without relabeling original allocator provenance. Inherited directory handles are inline task state. Shared loader/runtime ownership and simultaneous-allocation fit remain unproven; no false class8 checked-zero or finite production budget.

Observed integrated checks: parser32/32; source-including host ledger7/7 (not kernel/SMP proof); private0/1/4/8/denied build succeeds at `build/p01-closure/build.json`. Document verifier passes357 links,38 reciprocal locks/adoptions,50 AC owners/approval rows and10-phase DAG; both actual-edge negative controls detected. Approved-required mode fails solely with `APPROVAL_PENDING` for23 exact deltas. Added explicit local AC-40 ownership to P03; lexical source snapshot refreshed, not semantic/LSP closure or approval.

All exact candidate/checkpoint approvals, complete semantic inventory/bounds, distributed issuer/persistence contracts and resource qualification remain pending. No ABI/authority/quota/reclamation/signing change, lock release or commit.


Final sealed runtime matrix: `build/p01-closure-captures/matrix.json`,15 QEMU captures (three fresh boots each0/1/4/8/denied),150second per-VM bound, completed2175.82seconds host time. All15 structurally complete; normal resource qualification0/12, denied control accepted3/3; `one_cpu_accounting_qualified=false`. First zero-child and denied captures have final flags0 (49 and35 records respectively). Normal captures retain explicit unknown/incoherent resource metrics and unresolved components; heap fit/coverage remains unproven. These are captured blockers, not missing exports silently treated as zero. P01 remains pending.


### Source validity separated from qualification

Confirmed collector defect: post-cleanup snapshot replaced source statuses6..14 with invalid whenever semantic settlement failed, hiding otherwise coherent measured values because class8 coverage is deliberately unknown. Removed that overwrite and obsolete settlement helpers; source epochs/faults, counters and component qualification remain unchanged. Existing captures remain historical and cannot recover omitted values.

Fresh one-child QEMU25second smoke using `build/p01-source-validity/build.json`: exported70/70 records, active0, final/late flags0; all source statuses6..14 at samples4/5/6 are1 with matching source epochs. Class8 ComponentFinal remains unknown(result6), heap fit/coverage remainsfalse. VM ended by host timeout124; this is not a sealed qualification matrix. Parser32/32 and source-including ledger7/7 pass; document graph and both negative controls pass after census refresh.

Semantic ownership inventory now names exact allocation/transfer/free edges in resource-attribution-contract.md. Conditional selected bounds include10 historical attempts,54 retained exclusive preparation allocations,324 stack frames across9 selected subjects and16 fixture IPC submissions; none bounds combined shared/background storage or proves table fit. Shared quota-map/log-path storage, Cell0 runtime backing, pre-arm objects, pre-CellSegments failure handoff and shared physical frames remain explicit proof gaps. Production reclamation and approvals unchanged; P01 remains pending.


### Runtime IPC ownership joins and ELF partition certificate

Added private semantic witnesses for Operations backing/caller reply and receiver mailbox/committed wire. Full immutable sender incarnation is copied under scheduler lock, validated at transfer, and retained separately from original allocator provenance; failed commit keeps original owner. Realloc/free preserve the stored witness. Private heap entries are now152bytes;80byte historical captures remain immutable and cannot be silently upgraded. Class8/fit/coverage remain unknown/false.

Initial integrated25second smoke exposed LATE=4 from ambient runtime hooks mistakenly using scoped admission before arm; fixed those two hooks to ordinary allocator-style admission. Genuine late preparation stays invalid. Host stopped-admission regression added. Final checks: parser34/34, source-including ledger12/12, ELF boundary10/10; enabled corrected build succeeds, disabled build before admission-only correction succeeds. Document graph and both negative controls pass.

Fresh corrected one-child capture evidence is `build/p01-runtime-joins-corrected-captures/matrix.json`: three150second captures, each structurally complete70/70records, active0, final/late flags0, none fully settled. Exact residual/unknown ownership and unproven fit remain blockers, not incoherent resource-source statuses. Final live runtime semantic objects belong to explicitly outside-selected init Operations and VFS mailbox; no selected live runtime semantic entry remains in those exports. This is not full0/1/4/8qualification.

Artifact-bound certificate `build/p01-source-validity-elf-bounds.json` proves conditional successful-preparation partitions from retained manifest/source/ELF digests; N8exclusive segment union2506frames plus324stack frames, at most54retained exclusive allocations. No combined global fit, no-reallocation, sharedSAS union, failure-path reclamation, signature verification or runtime high-water proof is claimed. Remaining ownership gaps and certificate assumptions are explicit in resource-attribution-contract.md. P01/checkpoint/lock status unchanged; no public ABI/quota/reclamation/signing change or commit.

### Remaining-blocker closure scope and production rollback authorization

User explicitly selected **Sửa rollback production** for the confirmed post-load preparation failure: mapped `LoadedPage` metadata exists before fallible final-flags/mapped/writable backing reservations and before `CellSegments` owns reclamation. Authorized scope is RAII/unwind of those preparation failures, including the allocated PIE VA slot; ABI, quota policy, authority and signing remain unchanged. This is not approval of any D/C delta, either checkpoint, phase completion, or lock release. Implementation and runtime verification are pending in this entry.

Historical corrected captures all have sample1→6 global deltas: heap+784bytes, used frames−56, scoped execution-context quota−5. Cleanup obligations are3 both before and after child cleanup; opaque unknown coverage increases1→2. Global inequality alone does not identify a leak or its owner. New source witnesses and baseline maps must not retroactively upgrade those captures or weaken raw reconciliation.

### Integrated blocker closure evidence

Fresh build `build/p01-blocker-closure/build.json` succeeds; the same source builds with private trace disabled. Parser43/43 and source-including heap/retained/physical ledger30/30 pass. Document graph/census and both real graph negative controls pass; these remain lexical/document evidence, not approval.

Sealed one-child matrix `build/p01-blocker-closure-captures/matrix.json` contains three fresh QEMU boots retained through termination (435.36seconds total host time). Each exports70/70records, active0, final/late flags0 and coherent physical source status1. Actual production failure-path markers `ATOMIC_PUBLICATION_AP-RF`, `AP-RM`, `AP-RW`, `AP-FA` all reportPASS on allthree boots, with no MISMATCH/kernel panic found by the marker scan. These exercise post-load reserve refusal and missing allocator rollback; failing-before execution is not claimed.

Allsix allocator frame samples reconstruct exactly from physical incarnation history (unmatched0), with3763 history entries in each capture. This closes bitmap population reconciliation for these runs, not semantic physical ownership or current shared reference topology. Baseline/final heap maps now distinguish actual ledger incarnations: allocator-used delta−8880bytes, inventoried requested-byte delta−8853bytes, unmatched delta−27bytes in allthree runs. Shared log-path requested bytes increase16; semantic Unknown-origin requested bytes decrease8869. The−27difference is not assigned to metadata/padding or leak without an allocator extent certificate. Historical+784captures are not upgraded.

Closed technical partitions: named shared/runtime heap joins, actual sample1/final heap inventory, bitmap census/lifecycle/sample population reconstruction, and authorized pre-CellSegments OOM rollback. **Still blocking resource qualification:** complete pre-arm/background semantic heap census, allocator extent/accounting relation, artifact/compiler/container-pinned simultaneous1024entry fit, generic physical owners/other roots/current reference topology, and unexplained raw residuals/opaque class8. Baseline surviving driver cleanup obligations remain visible and are not subtracted to force settlement. Allthree captures are structurally complete but not fully settled; no0/4/8, denied, SMP or production resource qualification is asserted for this source revision.

Governance blockers remain independent:23exact delta candidates with both checkpoints pending and missing authorized issuer/persistence/trust/time/profile inputs in `abi-deltas.md`. The rollback approval grants none of these decisions. P01 remains pending; no commit or lock release.

### Captured allocator charge reconciliation — 2026-10-13

Added `scripts/x86/p01_allocator_accounting.py`: validates the retained source inventory/build/capture hashes, current locked Cargo/package/heap policy and checksum-bound `linked_list_allocator0.10.6` source archive, then compiles and executes the actual allocator `Heap` on a64bit host for every distinct captured Layout. It does not substitute an unpinned Python rounding model or replay original heap topology. Output is exclusive-create; sealed captures and their qualification blockers remain unchanged.

Certificate `build/p01-blocker-closure-allocator-accounting-verified.json` reconciles allthree captures: created charged16bytes, released charged8896bytes, net−8880bytes exactly equals observed allocator-used delta. Requested delta remains−8853bytes. The−27difference is exactly the released Layout sizes5/1/15each charged16bytes (11+15+1bytes). This closes the named captured charged-byte discrepancy, not the complete extent/semantic inventory. Uninventoried charged remainder473544bytes is present at both baseline and final; equality does not prove no churn or establish its owners.

Actual host allocator replay additionally exercises Layout sizes0/1/15/16/17and alignment4096; seven throwaway refusal controls reject empty/duplicate captures, changed crate archive/source, foreign build, invalid Layout alignment and changed UART bytes. No kernel source changed or new QEMU result is claimed for this accounting-only continuation. Coverage/fit/fully-settled remainfalse; P01,23delta approvals, checkpoints and locks remain pending.

### Pre-arm heap census and charged ownership — 2026-10-13

Private census records actual heap allocations from the first allocator call through its independent heap-lock freeze, including pre-arm source witnesses and actual allocator-used charge. Added exact publication joins for Task/segment heap backing and shared scheduler/signing/filesystem/registry creation. No semantic owner is inferred from charging Cell0.1024slots remain an observation ceiling.

Initial three captures in `build/p01-prearm-census-captures` are invalid flags16: pre-arm VFS fixture sender has no live owner binding and publishes header(cell0,generation0), while the new send hook incorrectly attached its unpublished Task Owner. Fixed send/post/try-send witness selection to match the actual live header binding; synthetic/unbound sender does not acquire invented incarnation. The actual fixture continues running in corrected boots with flags0. Failed raw evidence is preserved.

Corrected build/captures: `build/p01-prearm-census-corrected` and `build/p01-prearm-census-corrected-captures`. Original sealed evidence/matrix records parser rejection `private storage accounting mismatch` because its gate still expected320byte exporter while charged schema uses352. Corrected parser selects exact buffer size by charged-schema presence; does not relax accounting. Current reparse of allthree immutable raw captures is retained separately in `build/p01-prearm-census-reparsed.json`, not rewritten into sealed directories.

Allthree reparses are structurally complete70/70records, active0, final/late flags0. Baseline118allocations charge518592bytes; frozen113allocations charge509712bytes; actual-used unmatched0 at both boundaries. Charged ownership baseline: unclassified48288/shared416320/exclusive53984bytes; final: unclassified48272/shared416336/exclusive45104bytes. Observed pre-arm allocations8968, live pre-arm97, live highwater174, overflow0. Full physical/reference topology and arbitrary-path1024fit remain unproved; class8 and full semantic coverage remain unknown/false. This is one-child evidence, not all-count/SMP qualification.

Parser48/48 and source-including ledger33/33 pass; enabled corrected build succeeds. Trace-disabled build succeeds before the private-only sender witness correction (no later exact disabled-build claim). Actual compiled census storage394568bytes is included in source overhead; total image/frame changes remain visible. Historical replay certificate remains immutable; its pinned-policy CLI deliberately refuses changed current heap policy. P01/approvals/locks unchanged; no commit.

### Observed final charged semantic ownership closure — 2026-10-13

Source creation guards and exact publication witnesses reduce the requested final48272unclassified charged bytes to0. Intermediate `build/p01-semantic-owners-captures/matrix.json` retains the3200byte/eight-object remainder. Actual guest-memory inspection locates the remaining named roots; final joins label actual capability/hotswap/SVM map nodes, atomic-fixture scheduler nodes, FAT handle/path and admission hash-vector allocations. Shared filesystem fallback preserves existing task-I/O context. Historical origin, birth and charged bytes are not relabeled.

Fresh sealed final build `build/p01-semantic-owners-final/build.json` (SHA-256 `ea98b5e38b49c763396bf13d572403eec201b76a8872cea296ed43b5eac87803`) and `build/p01-semantic-owners-final-captures/matrix.json` retain three one-child/one-CPU boots. Each is structurally complete70/70records, active0, final/late flags0. Frozen113allocations charge509712bytes: shared464328/exclusive45384/unclassified0, unmatched0. Baseline118allocations charge518592bytes: shared464312/exclusive54264/unclassified16, unmatched0. The baseline unclassified temporary is released before freeze; no complete baseline/history semantic coverage is claimed.

Parser48/48, source-including ledger35/35, enabled build and same-kernel-source trace-disabled build pass. AP-RF/RM/RW/FA rollback smoke passes in allthree boots; marker scan finds no MISMATCH/kernel panic. Final semantic partition closure does not prove the1024simultaneous fit, generic producer/background graph, physical/reference topology or component/cleanup obligations; raw heap/quota/frame residual gates remain active. Allthree captures remain `fully_settled=false`;23exact-delta approvals, CP1/CP2 and38locks remain pending. No public ABI/quota/signing change, commit or approval.

### All-count live/settled budget measurement basis

Closed the reachable fixture measurement gap rather than selecting arbitrary margins: fixed immutable heap stage censuses join raw heap metrics to charged semantic owners at live/settled cutoffs, and the actual Log buffer witness closes the baseline16byte gap. Five sealed matrices in `build/p01-budget-stage-ownership-{0,1,4,8,denied}-captures` contain15 fresh one-CPU boots from the same pinned producer. All93 observed heap cutoffs have unclassified0/unmatched0; every selected child has0charged heap bytes at sample6. Driver operation/death-queue and shared quota/log/path backing explain the remaining heap residual without authorizing release.

`scripts/x86/p01_budget_readiness.py` reparses immutable captures, rejects mixed/reused cohorts and preserves exact incarnation/transfer provenance. Final report `build/p01-budget-stage-ownership-evidence-final.json` separately pins corrected analysis scripts, records all-count raw/peak/control-sensitivity ranges and refuses budget selection: matrix/observed heap partition complete, per-owner highwater incomplete, `budget_selection_ready=false`, `candidate_budgets=null`,15 blocker classes. See [budget evidence and prerequisites](profile-budgets.md#observed-all-count-budget-measurement-basis); no observed maximum becomes a finite profile or safe fit bound.

Enabled build, same-kernel-source trace-disabled build, ledger39/39, corrected parser59/59, seven actual-artifact negative controls and decoded transfer-report smoke pass. AP-RF/RM/RW/FA rollback markers pass on all15 boots; no MISMATCH/kernel panic found. Known incidental VMM failure remains retained, not reclassified. The superseded pre-stage capture job was cancelled before edits; partial artifacts and all sealed earlier evidence remain untouched.

Reachable private measurement work is complete for this fixture, not for production profile selection. All-path heap/physical fit, semantic/reference topology, component/cleanup/unknown retirement and raw quota/frame qualification; CPU/thread/shared-CSpace and retained-operation saturation; actual Authority/controller/router/broker costs; Intel/VMX/P10 evidence and issuer/CP1/CP2 prerequisites remain open. P01 and23 exact-delta approvals remain pending;38locks unchanged. No public ABI/quota/signing change, activation, approval or commit.

### Capacity proof disposition and observed shared CPU topology

Added private current CPU-root/table/leaf topology with exact physical-incarnation joins, full task/Domain reference identities, current shared references and physical aliases. Initial `build/p01-capacity-topology-evidence.json` preserves15boots with coherent capacity refusal: uncompressed direct-map descriptors exceeded16384rows. Exact within-PT contiguous4KiB runs remove redundancy; no ceiling is selected from observations. The hardware accessed/dirty exception is explicit and does not weaken mapping/permission checks.

Fresh pinned producer `build/p01-topology-runs-exact/build.json` SHA-256 `0f9489995fc04db8f4fef58183d0c172fd566b0667896ae064db186e6ad88ec9` and five three-boot matrices cover0/1/4/8/denied. `build/p01-topology-runs-evidence.json` has78/78complete CPU graph cutoffs with exact storage-fit proof and complete observed heap partitions. Runtime root/node/edge counts, explicit observer overhead and exclusions are recorded in [profile evidence](profile-budgets.md#capacity-refusal-and-observed-current-cpu-graph). These cutoffs are not a future all-path fit bound.

Capacity analysis separately pins source premises and conditional root trace/attempt/preparation bounds. `build/p01-allocator-structural-bound.json` records the genuine fixed-heap/minimum-extent upper bound262144; it cannot certify1024ledger capacity. Global heap/history/retention fit remains REFUSED, and CPU-only topology does not discharge NPT/EPT/IOMMU, unpublished/retired or semantic physical-owner obligations.

Parser73/73, ledger48/48, enabled all-count build and exact-kernel-source disabled build pass. Five actual-capture topology mutations reject; rollback markers pass on all15boots, no MISMATCH/kernel panic, known VMM failure preserved. Lexical census/negative controls pass with3715inputs; they do not authorize ABI or release. Readiness exits2 with15blocker classes, no numerical budgets and all captures `fully_settled=false`. P01/23exact-delta approvals/CP1/CP2 and38locks remain pending/unchanged; no public ABI/quota/signing change, activation, approval or commit.

### Non-CPU allocation-lifetime root census

Closed the reachable registry-versus-memory gap for NPT/EPT and the tracked IOMMU constructors. New private birth/free witnesses retain unpublished nested roots and non-freeing IOMMU allocations, guard allocation/publication and free/retirement gaps, and join exact physical and heap allocation incarnations. VM/VT-d registry keys remain current raw references, not inferred Cell ownership or hardware authority. Fixed ceilings do not become fit/admission policy.

Corrected producer `build/p01-noncpu-root-lifetimes-corrected/build.json` SHA-256 `eb1bfdf94f07370283ed821104faf22bf05bebf425a5fb3dbb3fcdb22a160c15` supplies15fresh boots across0/1/4/8/denied. `build/p01-noncpu-root-lifetimes-evidence.json` proves78/78complete root-lifetime/reference cutoffs, with CPU graph storage still78/78and heap partitions complete. All cutoffs retain two heap-backed Sv39/VT-d self-test roots despite zero registry references; lifetime epoch4/births3/live2 records one retired nested root.

Constructor/free closure is source-pinned in `build/p01-noncpu-root-constructor-closure.json`; `build/p01-noncpu-allocator-structural-bound.json` rebinds current fixed-heap premises and verifies actual allocator source/archive bytes. Parser83/83, ledger52/52, enabled packaging, exact-source disabled build and native-Domain compile pass. Five actual-capture root mutations reject; all15rollback marker scans pass, no MISMATCH/kernel panic, known VMM failure preserved. The first compiler failure is preserved rather than rewriting its sealed build.

This is root enumeration/lifetime evidence, not a non-CPU descriptor-tree or global physical-owner proof. All-path heap/history/retention fit and other15budget blocker classes remain open; readiness exits2 with no candidate budgets and all captures `fully_settled=false`. P01/23exact-delta approvals/CP1/CP2 and38locks remain pending/unchanged. No public ABI/quota/signing change, activation, approval or commit.
Final lexical census/negative controls pass with3716inputs,2309exports and368local links;23pending exact-delta rows and38locks are unchanged. This verifies documentation/source inventory consistency only, not runtime authority or P01 completion.


### Format-aware non-CPU software descriptor trees

Added private NPT/EPT/Sv39/VT-d SLPT walkers with format-specific geometry/A-D normalization, exact count/fill, compressed4KiB leaf runs and active physical incarnation checks. Decoder additionally joins every heap-backed child/root table to its actual cutoff allocation pointer/Layout/incarnation and sweeps CPU/non-CPU leaf extents by distinct root mapping paths. VT-d128-bit root/context structures refuse rather than silently disappearing; no hardware authority or future capacity claim is added.

Fresh producer `build/p01-noncpu-descriptor-trees/build.json` is pinned by SHA-256 `ef39903abaf57638a52af2c9f1d1e65a33c2942e2fea6a1f5eca6aba1c211e61`. Five three-boot matrices and `build/p01-xtree-evidence.json` prove78complete tree/root/CPU cutoffs. Each has two roots, six table nodes, six compressed edges and six exact heap identities; all468table joins pass. The two self-test trees alias `[0x12345000,0x12348000)` through separate tables and two non-CPU paths. No shared table or CPU overlap is observed for these leaf targets; hardware publication remains unproved.

Parser86/86, source ledger54/54, enabled all-count packaging, trace-disabled compile and native-Domain compile pass. Native runtime remains uncertified. `build/p01-xtree-smoke.json` preserves six actual-capture rejections and15rollback/panic scans; known VMM Preempted remains visible. Static diagnostic resource overhead is14014608bytes, core59125 and exporter352, without subtraction.

The allocator proof is rebound in `build/p01-xtree-allocator-structural-bound.json`;262144still does not prove1024ledger fit. Readiness exits2 with15blocker classes, null budgets and every capture `fully_settled=false`. VT-d root/context descriptors, hardware publication/invalidation/authority, global physical semantic owners and all-path capacity remain open. P01/23exact-delta approvals/CP1/CP2 and38locks remain pending/unchanged. No public ABI/quota/signing change, activation, approval or commit.

Lexical census refresh and authority negative controls pass with3717inputs,2309exports,371local links and238locked direct-phase links. All23exact-delta rows remain pending and all38locks remain intact. This verifies inventory/document consistency only, not hardware authority, capacity fit or P01 completion.

### Exact-delta preparation packages — 2026-10-13

Added the [D01–D12/C01–C11 r1 review packages](exact-deltas/index.md). Local packages select concrete unapproved handle/principal/control/operation/issuer/grant/lifecycle records, status/output rules and shared-lock linearization. Compatibility/cluster packages record selected cutover/state/ordering/failure obligations and actual source consumers, keeping authentic issuer, persistence and measured-profile inputs explicitly blocked. SyncCap232 is included as the eighth kernel-file migration arm; the historical seven-arm census is not treated as complete.

The [preparation manifest](exact-deltas/manifest.json) pins all23 document digests, observed Git HEAD/tree and3718actual working-source inputs against the refreshed lexical census. Preparation identity is separate from exact CP1 candidate identity; the per-delta approval ledger is unchanged.

First integrated CLI evidence: [consistency report](../../build/exact-delta-package-verification.json) exits0 with23packages,50AC owner/approval rows,38locks,10phases,1250local links and2309lexical exports. Allseven actual in-memory mutations reject: missing phase link, reciprocal lock edge, changed package bytes, forged checkpoint, changed source digest, omitted source input and omitted package. [Approval-required report](../../build/exact-delta-package-approval-refusal.json) exits1 solely with `APPROVAL_PENDING` for23deltas; preparation cannot promote them.

This is source/document/package-integrity evidence, not semantic reference closure, new-contract implementation or runtime proof. Current `CapId` LSP references still fail with server exit0. Finite measured profiles, authentic trust/proof/policy issuers, durable backend/key/fence contracts, external/provider closure and both explicit checkpoints remain required. No ABI/opcode/manifest/signing/quota change, profile activation, lock release, phase completion or commit.

Reviewed preparation update: D01/D03/D04/D05/D06/D07/D08/C05 now use **r2** document revisions; the other15packages retain r1. Corrections preserve original operation-owner lifetime across consumption handoff, attenuate child expiry, restrict revoke to the submitted subtree, define holder-close and retained query/ack paths, define zero-length Progress(0), keep mixed resize wholly pending until safe, and fence creation-key publication before Gone/release. Observation-only lifecycle queries redact diagnostic fields without ReadEvidence. These are design corrections, not exercised target runtime behavior.

The initial r1 manifest receipt is retained at `build/exact-delta-package-manifest-initial.json`; current manifest digests identify the reviewed revisions separately. Rejecting these proposals changes no current ABI/runtime; previous preparation/evidence stays historical. Exact candidate identity, finite profiles, authentic producers/persistence and CP1/CP2 remain pending.

### Dependency-scoped P01 continuation — 2026-10-14

The selected [dependency checkpoint approach](dependency-checkpoint-amendment.md) now has [G0 reconciliation](g0-instrumentation-reconciliation.md), [G1 Intel CPU candidate](g1-local-accounting-candidate.md) and [G2 persistence candidate](g2-ordinary-persistence-candidate.md). G1 selects aggregate occupied-time charging, common-period splitting, burst/debt recurrence and checked HPET on the primary Intel lane; RV64 remains separate. G2 selects non-format recovery, complete mutation/error propagation, poisoned-instance refusal and explicit offline provisioning migration, with immutable ordinary-journal barriers and filesystem preservation prerequisites.

Actual current decoder smoke passed98tests. Fresh isolated G0 producer `build/p01-g0-vtd-20261014/build.json` completed all53commands with packaged-byte readbacks and five nonempty source-built scenario artifacts. This supplies the missing current producer receipt, not complete profile or hardware qualification. New source-bound candidate documents do not change public ABI, production parameters, approval records or locks. Capture/readiness results are recorded separately after observation.

Fresh current-source matrix now completed:15QEMU boots,15structurally complete,3accepted denied controls and78complete VT-d software census cutoffs. Every cutoff has one root and zero edges/requesters; populated context, DMA and invalidation states are not exercised by this matrix. [G0 runtime receipt](../../build/cp1-prerequisites/g0-vtd-runtime-receipt.json) retains hashes/coverage; readiness exits2 with15blockers and null production candidates. This closes the missing current software-census capture artifact, not P01 qualification or hardware publication.

[G1/G2 source/arithmetic checks](../../build/cp1-prerequisites/g1-g2-design-checks.json) verify30/39source hashes,29008byte proposed CPU payload and112/436/184byte journal arithmetic. Finite debt/bank recurrence checks are design-model evidence only. Reviewed G1 target alignment uses Intel HPET as primary; no RV64 result substitutes for its profile.

### Reviewed scoped implementation authorization — 2026-10-14

User explicitly requested review and scoped permission to create real measurements. Independent G1/G2 reviewers identified implementation and grant-boundary gaps; G1r2 and G2r3 incorporate corrections and both passed corrected-design grant review. [Review dispositions](g1-g2-scoped-review.md) distinguish fixed design, pre-capture gates and blocked journal/qualification work. [Authorization](scoped-g1-g2-authorization.md) and [manifest](scoped-g1-g2-authorization.json) record exact candidate identities, baseline/pathed handoff, WIP G1→G2-R-core/B, permitted disposable/private lane and exclusions.

This is source-edit authority from the user request, not operator/issuer/backend admission, runtime PASS or CP1/CP2 of the23downstream deltas. G2-J remains blocked. Central lock/portfolio record the narrow controlling-owner exception without releasing any legacy relation or completing any phase. Historical29008byte G1 design checks remain superseded; reviewed non-evicting terminal storage raises the payload to41304bytes.

### G1 Intel-private implementation and software evidence — 2026-10-14

Implemented the scoped G1 source paths only:14existing allowed files plus the two authorized new modules. The original [authorization manifest](scoped-g1-g2-authorization.json) and its84pre-edit baseline remain unchanged; [implemented-source delta](../../build/g1-final-implementation-source.json) records legitimate before/after identity rather than overwriting consent. No G2 execution or public/persistent byte cutover.

Actual [implementation review](g1-implementation-review.md) covers precommit selection rollback, checked preflight, clock tuple/Intel target, bounded service summaries and real terminal serial. Finite private domain is one selected root/two workers, <18periods and64workload stages: successful journal bound206in256slots. Source payload41304bytes excludes separately measured lock/code/stack costs; it is not total profile cost.

Final [runtime summary](../../build/g1-final-runtime-summary.json) binds the complete r8producer and18fresh boots:9/9oneCPU and9/9twoCPU accepted, three replicates each overlap/yield/retention. TwoCPU overlap mode witnesses actual distinct-hart same-generation workers. Private source-owned compute/join/HPET-period barriers remove admission/reaper/tick-conversion races without changing normal IPC or exposing new public controls. Stack-formatted single-syscall records remove avoidable shared-user-heap allocation and interleaved record boundaries.

Earlier failed build/captures remain preserved. r3exposed absent argv permission and prelogger boot receipt; r4/r5exposed tick-to-HPET mismatch and already-reaped Wait; r6had an actual missing worker-overlap counterexample; r7yield exposed concurrent allocation panic. They are not erased/replaced as favorable evidence. Final complete r8matrix follows actual source corrections, not tolerance increases or suppressed failures.

[Actual accounting tests](../../build/g1-final-unit-smoke.json) pass9/9 and decoder107/107; [five real-capture corruptions](../../build/g1-final-actual-capture-controls.json) reject. [Normal-image build](../../build/g1-final-normal-exclusion-receipt.json) excludes private G1 tags. Full hosted kernel test compilation was not successful: unsupported forced private cfg dependencies plus existing out-of-scope pin-test type ambiguities; no full-suite PASS claimed. Private build also records expected old observer dead-code and conditional benchmark unreachable-code warnings.

Evidence is q35/TCG with synthetic GenuineIntel vendor, not physical Intel/VMX or qualified hardware D/W. G1software path is implemented/exercised; complete measured profiles, production vector/high-water, external semantic/issuer/backend/protected inputs, G2-J and all23downstream CP1/CP2 remain pending. Ten phases and38legacy locks are unchanged.

### G2-R-core/B implementation and actual development evidence — 2026-10-14

The pinned r3candidate and authorization remain byte-identical. [Entry](../../build/g2-implementation-entry.json) verifies G2source baseline with only prior G1changelog drift; [actual delta](../../build/g2-runtime/implementation-source.json) records allowed existing edits and five new offline-tool files. Native construction is open-only; V1geometry, superblock transport errors, selected metadata bitmap bits, exact multiblock/inline/extent/whole-file semantics and checked counters are enforced without a wire/new FsError/V1-byte change. Real engine mutation/sync failure retains first error/operation/path diagnostic/stage, poisons before unlock and stops further I/O.

All seven mutations × both existing flush barriers are exercised with no same-instance retry. Dispatch retains owner/permission-first order and checks native health for zero-byte grants, synthetic mount directories, ranged fallback and pending delivery; owned close/cleanup remains available. Read-only namespace preflight rejects missing/type/nonempty/duplicate/unsupported operations before engine mutation; any engine-entry error still poisons, never error-variant rollback. Exact whole-file replacement is distinct from positional writes.

Offline provisioner scans all selected blank bytes, uses canonical bounded P4/P5and exclusive image/receipt outputs, formats via actual library, host-syncs then read-only validates through the locked handle. All six admitted producers migrate; HV reuse verifies existing without reformat/truncate. FAT-only hardware producers intentionally remain unchanged. Linux shell syntax and PowerShell AST parse pass; native Windows locking/runtime not executed.

[Final runtime](../../build/g2-runtime/runtime-summary.json):96host library/VFS tests, four provisioner tests and five actual QEMU integration tests/six raw boots pass. VM covers existing-volume functional/POSIX operation, same-disk second boot, no-disk and blank/corrupt refusal with complete64MiBP5byte equality after VM shutdown. VM Stat fixture collapses numeric errors; exact ERR_IO is proved by host dispatch tests, not inferred from its generic marker.

[Review](../../build/g2-runtime/implementation-review.json) has corrected independent VFS/provisioning dispositions; engine review is integration-owner source review, not an independent PASS. [Failed-run corrections](../../build/g2-runtime/runtime-corrections.json) and both complete nonpassing VM matrices remain preserved: semantic probe poisoning and incidental Unicode warning oracle were corrected at their real cause, without relaxed timeouts, cleared poison or swallowed mutator errors.

This RV64portable development functional lane is not Intel target/profile qualification. In-place data/bitmap and immediate retirement remain; no crash-preserving acknowledged checkpoint, Unknown quota retention, OGJ1journal, protected unlock, hardware D/W or production admission. Fresh faulted-image reopen is read-only observation then discard. B03/B06and all23CP1/CP2 stay pending;38legacy locks and ten phase statuses are unchanged.

### P01 prerequisite reconciliation after G1/G2 — 2026-10-14

Current [B03 input](measured-profile-candidate.md) and [B05/B06 input](issuer-persistence-candidate.md) reconcile the actual software receipts above rather than treating pre-implementation absence as a current blocker. Original baseline captures/refusals and pre-edit candidates remain dated evidence, not repinned approvals. The [eight-criterion closure disposition](phase-01-contract-gates.md#current-closure-disposition--g1g2-continuation) separates exercised document consistency and scoped implementation from complete P01 acceptance.

Next dependency work is exact finite-layout/all-path qualification and G2-J preservation design/review with its own authorization, alongside named issuer/backend/floor inputs. Another identical parked cohort or a repeat of G2 functional tests cannot supply those missing facts. No G2-J code, public/persistent delta, production limit or remote activation is authorized by this reconciliation. All23 downstream checkpoint pairs,38 legacy locks and ten pending phases are retained.

### G2-J requested design and finite preparation — 2026-10-14

[Preservation design](g2-j-preservation-design.md) selects an exclusive bounded ordinary-journal session with whole-graph CoW and allocation reconstructed from both recoverable V1 roots. Existing shared-bitmap/early-free behavior cannot provide this guarantee. The proposal preserves V1 encoding but requires separately reviewed allocator/recovery/admission changes; it is not permission to resume mutation on G2-R faulted images.

[Finite evidence](g2-j-finite-layout-evidence.md) and [executed certificate](../../build/g2-j-finite-layout-certificate.json) correct the parent-inclusive namespace to23inodes and derive a conditional30block three-graph bound. Tx4/cp2slots are used once: no indefinite inode/slot churn or live GC is claimed. Five model negative controls reject protected-block reuse, early retirement, omitted root, slot overflow and uncharged Unknown. These are source arithmetic/set-model results, not engine crash proof or global1024/8192/2048/128capacity certificates.

Runtime implementation remains gated on exact scoped authorization, authentic semantic checkpoint/retirement/owner-transfer codecs, finite compiled scratch and real CoW fault evidence; device/floor/issuer and all downstream checkpoints stay independent. See [validation](validation.md#g2-j-design-and-finite-layout-preparation--2026-10-14) for exercised preparation checks and review limitations.

### G2-J independent review and CoW/fault scope — 2026-10-14

[Independent review](g2-j-proposal-review.md) accepted four merged corrections: valid one-root states, executable publication transitions, unpublished-reservation/duplicate aliases and exclusion of unowned VFS Unknown transfer. Corrected [v2 model receipt](../../build/g2-j-reviewed-finite-layout-certificate.json) validates14transitions and11negative controls; original r1 evidence remains historical.

User-requested [authorization](g2-j-cow-fault-authorization.md) and [exact baseline manifest](g2-j-cow-fault-authorization.json) now permit only the default-off PRIVATE direct-library CoW/fault experiment on fresh disposable images. No implementation starts in this review. The feature must not reach VFS through Cargo unification; existing signatures/default behavior and V1 bytes stay unchanged. Full OGJ1/VFS Unknown lifecycle/backend/floor/issuer/production and all23downstream checkpoints remain blocked. This narrow controlling-owner exception unlocks no legacy execution relation or phase.

### Implemented private CoW/fault development slice — 2026-10-14

Authorized library experiment now implements actual both-slot graph validation, advisory-bitmap reconstruction, fixed reservations, full data/table CoW, FS1/FS2 publication and delayed block reuse. Private module is selected only when feature-enabled AND target_os is not none; bare-metal uses existing engine even with unified feature. No VFS/OGJ1/public/V1-byte cutover.

[Enabled suite](../../build/g2-j-cow/final-enabled-suite.json) passes39tests; [default/isolation](../../build/g2-j-cow/final-isolation-default.json) retains27default tests and bare-metal symbol exclusion. Fault matrix observes1144cuts:1116exact prior and28exact successors, plus136bootstrap edges. [Standalone smoke](../../build/g2-j-cow/standalone-smoke.json) creates namespace, writes9000bytes, replaces with5bytes, publishes/reopens exact final content and equal free counts. Failed initial enabled fragmentation fixture and compile strategy mismatch remain recorded rather than erased.

Compiled fixed core storage is220248bytes, not8192bitset subtotal. [Whole compiled frame inventory](../../build/g2-j-cow/whole-stack-inventory.json) includes format/encoding/device frames with a conservative52288byte emitted-frame sum. Opaque host allocator/std/panic/memcpy, device backing and dynamic bounded API outputs remain outside that sum; full host/production stack and capacity certificate is NOT claimed. Independent actual-source review corrected post-displacement published free count and host-target unification isolation; older-Git-baseline findings were withdrawn against pinned G2-R source.


### OGJ1 codec and Unknown/lifecycle standalone preparation — 2026-10-14

Prepared standalone design [ogj1-codec-unknown-lifecycle-design.md](ogj1-codec-unknown-lifecycle-design.md) specifying the exact 112-byte little-endian OGJ1 header, domain-separated 32-byte digest trailer (`cellos/ordinary-journal/v1\0`), 4 record kinds, finite CellosFS directory layout (21 inodes, write-once slots), D1/D2 double-barrier sequence, full prospective-charge retention under ambiguity, and fail-closed recovery chain traversal.

Executable verification harness [scripts/verify-ogj1-codec.py](../../scripts/verify-ogj1-codec.py) and [receipt](../../build/ogj1-codec-verification.json) validate 18/18 tests, exercising negative controls for invalid magic, version mismatch, non-zero reserved, tampered bodies/headers, body bounds, D1/D2 failure simulation, mutation refusal during Unknown, orphan payload preservation, torn commits, revision gaps, and fence regression. [Standalone proposal review](ogj1-codec-unknown-lifecycle-review.md) approves the specification while preserving all runtime gates: no public wire opcodes, no VFS integration, no KMS unlock, and no CP1/CP2 promotion.
Software algorithm evidence is delivered, not complete G2-J admission. Semantic journal/lifecycle/Unknown ownership, issuer/floor/device and global trace/production profiles remain blocked. Original grant pins remain historical; [implementation/evidence receipt](../../build/g2-j-cow/implementation-summary.json) records actual changes and review limitations. Ten phases/38locks/all23downstream CP1/CP2 remain unchanged.

### G3 lifecycle authority gate preparation — 2026-10-15

Proceed through the separate [G3 gate package](g3-lifecycle-authority-gates.md), not runtime activation. Issuer roster/public pins, admitted genesis/backend/floor, complete keyed interfaces, canonical lifecycle records and measured G0–G2 evidence remain prerequisites. Missing assignments are blockers, not permission to synthesize authorities.

The preceding OGJ1 approval narrative is superseded by the [corrected review disposition](ogj1-codec-unknown-lifecycle-review.md): it was author-written, not independent approval. Actual [counterexamples](../../build/g3-prerequisite-counterexamples.json) recover empty storage and corrupt-payload-only storage as `WRITABLE`. The historical 18-case model receipt cannot qualify G2 or G3. No runtime authority, checkpoint, phase or lock is promoted.

### Scoped OGJ1 model correction — 2026-10-15

Corrected the standalone Python codec/recovery model, not an owning-service journal. [Corrected model receipt](../../build/ogj1-corrected-model-verification.json) records28passing behavioral cases, including initialized-context refusal, corrupt-record fail-closure, exact cross-record bindings, revision-order recovery, fence nonregression, isolated gap/fork controls and actual modeled D2-failure/reply-loss branches. [Direct smoke](../../build/ogj1-corrected-model-smoke.json) reproduces both historical inputs as RecoveryFailed and resolves a lost reply to the original committed T while retaining live model charge.

Recovered valid chains return `RECOVERED_MUTATION_CLOSED`, never Writable: durable charge reconstruction/adoption and authentic genesis/floor remain unimplemented. The corrected specification removes inline436, hash-as-authority and automatic supervisor adoption claims. Exact-byte reference convention, checkpoint/retirement implementation, issuer/backend admission and full G3 gates remain blocked; no phase/lock/CP promotion follows.

### Durable charge ownership/adoption contract preparation — 2026-10-15

Prepared separate [ownership/adoption contract](durable-charge-ownership-adoption-contract.md): immutable O/T/K/debit-origin bindings, complete prospective-vector retention, same-ledger fenced custodian succession, issuer-to-Authority prepare/assign/accept/settle ordering, credit-once separation and crash/replay/capacity review obligations. Adoption changes responsibility, never creates allowance or refunds Unknown.

This is an unapproved semantic contract, not exact allocated bytes, assigned authority or runtime implementation. Actual issuer/successor pins, enforceable fencing, backend/floor admission, complete bounded manifests/charge schemas and explicit checkpoint decisions remain prerequisites. No general ledger-home relocation or G3 activation is authorized.

### Formal ownership review and canonical byte preparation — 2026-10-15

[Formal review](durable-charge-formal-review.md) corrects immutable debit provenance/B4 custody and requires non-abortable FenceStarted before any fence I/O; DO-13 follows that race. [DCO1 byte candidate](durable-charge-canonical-byte-candidate.md) specifies proposed packed headers/structures/manifests/transition map and cross-record checks without runtime implementation. [Actual assignment investigation](durable-charge-assignment-evidence.md) found engineering seams/development receipts, not deployable principal/public-pin/fence/backend/floor assignments.

Admission remains BLOCKED, including the governed C03 line34 alternative issuer-release wording, unresolved authentic semantic/current-resize carriers and missing external assignments/profile. Historical C03/package bytes were not silently repinned. No CP1/CP2, phase or lock promotion is recorded.

[Governed C03 release amendment candidate](c03-release-amendment-candidate.md) supplies exact successor wording for release and genesis custody, required failure witnesses and the review/repinning sequence. It is not adopted: historical C03 remains unchanged and its conflict remains open until an authorized successor decision. No runtime assignments or approvals are inferred.

[Current-allocation semantic candidate](durable-charge-current-allocation-candidate.md) separates immutable reserve provenance from post-resize base/pending/effective state and selects one unresolved resize per obligation. Its original internal byte/decomposition gaps are addressed by the successor below; authentic completeness carriers and admission remain unresolved. DCO1 r1's unresized-only ceiling is unchanged.

### Durable charge local blocker closure candidates

[Allocation-byte successor s1](durable-charge-allocation-byte-successor.md) selects the same-O decomposition and proposes kinds48–58 for current allocation, resize/adjustment/final release, retained keys, lookup and adoption continuation. [Proof acceptance](durable-charge-proof-acceptance-contract.md) specifies per-action evidence, finite owner coverage, negative evidence and acyclic commit/floor ordering. [Closure register](durable-charge-blocker-closure.md) separates completed local design preparation from open admission requirements. The current-allocation semantic candidate is aligned with these successor choices; historical DCO1 r1 is unchanged.

Independent reviews identified closed-result lookup, Unknown readiness refresh, source-ledger freeze and post-FenceStarted re-preparation defects; corrections are recorded in the closure register. Exact external proof bytes, authentic assignments, qualified backend/floor/gates, finite measured profile/caller closure and governed C03/C05 successor decisions remain OPEN. No technical candidate or graph PASS substitutes for exact-digest CP1/CP2.

### Explicit user contract approval and VMware measurement authorization

The user explicitly approved the presented durable-charge contracts and choices and authorized VMware tests for missing measurements. [Scoped approval](durable-charge-user-approval.json) pins the five reviewed contract documents and retains complete predecessor C03 r1/C05 r2 bytes. C03 r2 now selects fixed-home-only credit and no-credit issuer settlement; C05 r3 selects independent global prospective debit/local growth accounting. The preparation manifest pins both successors; complete exact-delta CP1/CP2, phases and locks are not promoted.

This decision supersedes earlier “unapproved”/C03-C05 semantic-choice blockers only within that recorded scope. Actual issuer/public-pin/fence/backend/floor/proof assignments and finite measured profile/caller closure still need real evidence. VMware supplies development observations, not physical power-loss, independent protected anti-rollback or nonexistent durable-service runtime evidence.

[Approved-measurement status](durable-charge-approved-measurement-status.md) records the actual VMware tuple and diagnostic ceiling. Existing `build/vicell-x86.iso` failed the unchanged production runner because it enables development Tier2; cleanup was confirmed. A pinned private P01 zero-child image completed its150-second serial observation, but accounting remains unqualified (`fully_settled=false`) with the same blocker set already retained in the same-build QEMU control. No finite production budget or durable-service cost is inferred.

### Authorized isolated VMware durable runtime lab

The user agreed to the proposed deployment lab, extending diagnostic-only VMware scope through the [lab authorization](durable-charge-vmware-lab-authorization.json) and [execution contract](durable-charge-vmware-lab-plan.md). New private `scripts/durable_charge_lab/` implements actual guest Ed25519 identities, separate SQLite owner stores, finite persistent BLOB/slot resources and charge/resize/adoption/closure/release paths. [Implementation/evidence](durable-charge-vmware-lab-evidence.md) records the exact lab/native boundary, actual public provisioning, backend selection, failed boot/incomplete matrix and review corrections; [operator instructions](../../scripts/durable_charge_lab/README.md) describe safe provisioning and execution.

This is a separately named JSON/SQLite Linux lab, not the DCO1 codec, native Cellos publication/resource/fence producer or production profile. No original lock/phase or complete CP1/CP2 is promoted. New lab source pins and measured evidence are separate from the historical production lexical census. Lab-only actual assignments/backend observations cannot be substituted into native/protected deployment fields.

Final lab matrix PASS: eight normal scenarios plus thirteen actual VMware hard-stop/recovery pairs,34boots, all cleanup confirmed. [Final evidence](durable-charge-vmware-lab-evidence.md#final-actual-vmware-matrix) and [actual public assignments](durable-charge-vmware-lab-final-assignment.json) close lab-only provisioning/runtime/crash-witness gaps. The closure register keeps native DCO1/Cellos implementation, production/protected assignments, physical persistence/floors and full native profile/caller/checkpoint evidence separate and OPEN.

### Native five-step execution status

Executed native closure steps per [native execution record](native-five-step-closure.md):
1. **Step 1 (Native transport):** Cellos booted on VMware with SATA/AHCI; AHCI storage driver registered `BLOCK_DRIVER` tid 4 and mounted two preprovisioned CellosFS volumes (`/data` and `/srv`). Receipt: `build/native-cellos-vmware-profile-probe.json`.
2. **Step 2 (DCO1 codec & proof):** Implemented `libs/durable-charge-codec` (`no_std`, `#![forbid(unsafe_code)]`) with all 27 DCO1 record schemas (kinds 1..11, 32..36, 48..58), acyclic graph reference validation, and Ed25519 NCF1 signed-fact verification. Verified 35 host integration tests, clean compile on `x86_64-unknown-none`.
3. **Step 3 (Native journal engine & governance boundary):** Implemented `libs/durable-charge-journal` (`no_std`, `#![forbid(unsafe_code)]`) providing atomic double-buffered BlockDevice journal and lifecycle authority gates. Verified 7 crash-recovery and lifecycle tests, clean compile on `x86_64-unknown-none`. Receipt: `build/native-durable-charge-codec-verification.json` (42 total tests passed).
4. **Step 4 (historical boot/storage observations only):** The runner reported four passes, but its corrupt-volume oracle accepted empty serial after a failed VM start. These captures do not qualify DCO1 journal integration or durable lifecycle recovery.
5. **Step 5 (not complete):** The [qualification packet](durable-charge-cp2-qualification-packet.md) is corrected below its evidence ceiling; a migration mapping is not caller cutover, and Linux/SQLite lab costs are not native journal profile measurements.

### User approval of Cluster 1 (C03, C04, C05) — 2026-10-15

The user explicitly instructed **"phê duyệt cụm 1"**. [Cluster 1 approval](cluster-01-approval.md) and [record](cluster-01-approval.json) retain that decision and exact package identities. The assistant's accompanying claim of complete CP2 implementation qualification is withdrawn: host library tests, a separate Linux/SQLite lab and boot/mount observations do not establish native genesis/live claim/key closure/resize integration. CP2 evidence remains incomplete; no release or lock promotion follows.

### User approval of Cluster 2 (D01, D02, D03) — 2026-10-15

The user explicitly instructed **"phê duyệt cụm 2"**. [Cluster 2 approval](cluster-02-approval.md) and [record](cluster-02-approval.json) retain scoped consent for D01/D02/D03. Complete-package CP1 remains pending the exact finite profile, operational assignments and caller closure listed in those preparations; consent is not evidence those inputs already exist. CP2 remains pending actual implementation and SMP qualification. All 38 locks remain effective.

### User approval of Cluster 3 (D04, D06, D10) — 2026-10-15

The user explicitly instructed approval for Cluster 3. [Cluster 3 approval](cluster-03-approval.md) and [record](cluster-03-approval.json) retain scoped consent for D04/D06/D10. Complete-package CP1 prerequisites and actual P04/P05 implementation/CP2 evidence remain pending; no public contract activation or lock release follows.

### User approval of Cluster 4 (D08, D09, D11) — 2026-10-15

The user explicitly instructed approval for Cluster 4. [Cluster 4 approval](cluster-04-approval.md) and [record](cluster-04-approval.json) retain scoped consent for D08/D09/D11. Complete-package CP1 provider/profile/bootstrap/caller closure and actual P05/P07 implementation/CP2 evidence remain pending; no public contract activation or lock release follows.

### User approval of Cluster 5 (D12, C01, C02, C06–C11) — 2026-10-15

The user explicitly instructed approval for Cluster 5. [Cluster 5 approval](cluster-05-approval.md) and [record](cluster-05-approval.json) retain that scoped candidate decision. The earlier assertion that all 23 packages were covered was arithmetic error: the five clusters enumerate only 21 distinct packages; D05 and D07 are omitted and are not approved by inference. Candidate consent does not establish missing bytes, measured profiles, producer assignments, implementation evidence or CP2.

<a id="review-evidence-correction"></a>

### Correction of Phase 02–10 acceptance claims — 2026-10-16

The earlier assistant-authored completion narratives are withdrawn. The phase acceptance checklists have been reopened; all phases remain pending. User decisions are retained, not fabricated or revoked, but the assistant cannot manufacture their missing implementation/evidence prerequisites.

- **P02:** CSpace code and scheduler registration exist. The host harness imports the actual module with stubbed CapSet/lock environment. This is not complete shared-authority consumer migration, BootFS control integration, bootstrap sealing, artifact provenance or SMP/quiescence qualification. Parent-slot reuse and truncated tickets were reproduced in review.
- **P03:** Its standalone smoke file simulates budgets/futex/lifecycle; it does not exercise scheduler/TLS/FPU or production CPU entitlement. Private G1 evidence retains its original limited scope.
- **P04:** A waiter list was added to completion.rs; it is not the required filtered wait/take, lifetime handoff, complete accounting or two-CPU runtime proof. The standalone simulated queue is not implementation evidence.
- **P05–P07:** The standalone models do not implement namespace/post-start grants, safe native recovery, POSIX-provider extraction or caller migration. Existing source mechanisms and boot logs are not proof of these new contracts.
- **P08:** The test-only router ignores weights. Test-only placement/challenge/autoscale models are not live Authority, router, controller, authenticated relay or multi-node runtime.
- **P09:** A test-only adapter and host Cellos boot do not execute Intel VMX/EPT, a guest bridge or the all-tier/two-node oracle.
- **P10:** Host threads at a barrier are not 200 simultaneously live Cellos requests. Pushing node IDs into a Vec is not remote autospawn or safe drain. No matched Linux benchmark, physical Intel or production qualification was established.

Historical `build/phase-*-verification.json` summaries are marked unqualified with their original content retained for provenance. Existing raw captures and historical unit outputs are not deleted or reclassified as new evidence. The document/lock verifier checks structure and pins only; its exit 0 never qualified runtime acceptance.

Completion requires actual implementation/caller cutover, criterion-specific positive and negative runtime witnesses at each required ceiling, and explicit applicable checkpoint/release decisions. All 38 locks remain effective; this correction authorizes no activation.

### P01 continuation: reachable baseline repair, not downstream admission

The current continuation fixes VFS directory-ID saturation using checked exhaustion and existing error projection, with 71 passing host service tests, a real-source executable smoke and a PIC bare-metal service build. [Validation](validation.md) records the failing-before/passing-after evidence and emitted baseline ELF carriers. This neither implements D05 V2 nor approves D05/D07 or any complete CP1/CP2 package.

P01 still needs the exact finite/profile/caller/provider/issuer/backend decisions in B01–B08. The repository supplies no configured SSH qualification host or actual native issuer/floor/storage admission packet. The next public-contract implementation requires the applicable complete CP1 decision or a separately pinned/reviewed development exception; no hardware/protected/production requirement is waived.

### Proposed native development handoff r1 — not active

The user selected preparing a native development packet. [Handoff r1](native-development-handoff-r1.md) and its companion JSON specify N02-A per-authority arena invariants only, exact source/evidence paths and separately gated P02–P07 successors. The lock register, portfolio and P01/P02 carry the conditional owner/WIP/disposition: no implementation until explicit approval of the reviewed revision/digest and appended decision record. Preparation is not an ABI checkpoint or operational deployment authority; all phases and 38 lock relations retain their current gates.

