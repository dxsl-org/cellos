---
title: "SAS/LBI Closure and Lab-First Robot Workflows"
description: "Approved native correctness closure, followed by LAB-01 carrier transfer and gated BASE-01/ASSEMBLY-01 outcomes."
status: in-progress
priority: P1
effort: ""
tags: [architecture, sas, lbi, performance, reliability, lab, robotics]
blockedBy: []
blocks: []
created: 2026-09-05
---

# SAS/LBI Closure and Lab-First Robot Workflows

## Approval and Decision
User approved Approach A in the architecture brainstorm on 2026-09-05 (`duyệt`). Keep trusted native SAS, mediated typed IPC/grants, generation-aware retirement and selective containment. This artifact is the planning handoff, not implementation or qualification evidence.
Reject a whole-system direct-vtable rewrite and per-Cell private roots as the default. Fast paths require measured benefit; Tier 2/remote/production remain governed lanes.
User subsequently approved lab-first focus and requested concrete lab, base/leg and assembly workflows in this same plan. [ADR-0014](../../docs/decisions/0014-lab-first-robot-workflows.md) records that product-scope decision. All robot hardware remains on paper; this does not grant physical actuation, procurement, a public ABI delta, or production approval.

## Scope Contract
- M0: valid performance evidence and truthful current architecture/roadmap projections.
- M1: task-local grant-owner semantics, investigate then fix proven quota attribution, and state-required hot-swap with rollback.
- M2: fresh reproducible baseline and a measured bottleneck/footprint decision, not an unapproved allocator rewrite or target reduction.
- M3: native stateful counter + real VFS checkpoint/readback + hot-swap and service-restart behavior, using existing demos/SDK/harness.
- M4: retain explicit reopening gates only; no speculative implementation phase.
- LAB-01: transfer one closed inert dummy carrier with ID from indexed rack A to rack B, confirm placement and record a traceable result. Phase05 remains engineering evidence, not physical sample transfer.
- BASE-01: carry a secured dummy-carrier tray between two named test handoff stations, only after exact base/controller/stability/safety readiness.
- ASSEMBLY-01: stationary coupling/uncoupling, explicit configuration validation and re-enable; run LAB-01 while assembled and stationary and qualify independent modes separately.
- One active workflow job is shared across LAB-01, BASE-01 and ASSEMBLY-01; composed steps retain its identity, and other requests are rejected until completion or explicit reconciliation closes it. Command acceptance is not physical completion. Uncertain effects require reconciliation, not blind replay after snapshot/VFS recovery. Arm manipulation and base transport never overlap; stationary/support/lock eligibility remains fresh through carrier retention and arm parking.
- ORG-SRV-01 and ORG-PC-01: [bounded organizational server and office-PC profiles](./organization-deployment-profiles.md), superseding the specialist-device suggestion for organizational replacement. G1 and G2 are independent product lanes, not sequential phases: G2 does not wait for robot hardware or G1 completion, and G1 does not wait for G2. Each consumes only its own documented prerequisites plus shared-platform changes it actually uses. Their scope is defined; this amendment does not activate a concurrent G2 implementation program.
- All required outcomes remain in scope. A failed prerequisite is BLOCKED with its exact cause, never a smaller PASS or a silently dropped phase.

## Phases
| Phase | Outcome | Status | Depends | Ceiling |
|---|---|---|---|---|
| 01 | [Valid measurements and truthful projections](./phase-01-evidence-validity.md) | completed — collector fail-closed; fresh QEMU capture binds exact source patch | — | host/QEMU |
| 02 | [Grant ownership and quota integrity](./phase-02-resource-ownership.md) | completed — task-local owner handle; quota drift fixed | — | host/QEMU |
| 03 | [State-required service replacement](./phase-03-stateful-hotswap.md) | scope-gated — authenticated transaction/principal ABI delta required | — | QEMU |
| 04 | [Native performance and footprint baseline](./phase-04-native-baseline.md) | completed — source-bound native scorecard and footprint candidate established; VFS rows unblocked | 01; 02/03 per affected row | QEMU |
| 04b | [CellosFS Native & Vector Block DMA Engine](./phase-04b-cellos-native-fs.md) | completed — pure-Rust CoW extent engine replacing RedoxFS/LittleFS; 10k power-cut tests + QEMU two-boot persistence verified | 02 | host/QEMU |
| 05 | [Stateful native workload outcome](./phase-05-native-workload.md) | completed — 1000-op stateful workload with real VFS checkpoints, v1->v2 hotswap, and VFS restart recovery verified 3x in QEMU | 02,03,04b + consumed 04 rows | QEMU |
| 06 | [LAB-01 dry carrier transfer](./phase-06-lab-carrier-transfer.md) | partial — 06A host contract and 06B QEMU native witness completed; 06C external-gated | 06A independent; 06B consumes 05 | contract/host, then QEMU; physical gated |
| 07 | [BASE-01 tray handoff](./phase-07-base-tray-handoff.md) | partial — 07A host contract and 07B QEMU native witness completed; 07C external-gated | 07A consumes 06A; 07B consumes 05 | contract/host, then QEMU; physical gated |
| 08 | [ASSEMBLY-01 stationary integration](./phase-08-stationary-assembly.md) | partial — 08A host contract and 08B QEMU native witness completed; 08C external-gated | 08A consumes 06A + 07A; 08B consumes 06B + 07B | contract/host, then QEMU; physical gated |
Frontmatter dependencies represent whole-phase gates; Phase 04b unblocks the real VFS fixture for Phase05 and canonical VFS rows in Phase04.

## Architecture Discovery & Brainstorm

Comprehensive architectural root-cause analysis and roadmap strategy under `hl-brainstorm` (Architect, Scientist, and Devil personas) is documented in [sas-lbi-architecture-root-cause-analysis.md](./sas-lbi-architecture-root-cause-analysis.md). It establishes the dual-mode kernel evolution (Real-time SAS Tier 1 + Paged Domain Tier 2) and the 4-stage hardware/memory recovery gates.

## Workflow Milestone Gates
Frontmatter `dependencies` remain whole-phase gates. New phases have no whole-phase prerequisites: drafting need not wait for entire preceding physical phases. Milestone gates below are mandatory, including shared-contract gates for 07A/08A implementation and acceptance. Drafting may overlap under WIP/file ownership; it cannot accept duplicated or unfrozen contracts. A software PASS never completes the workflow phase while its physical outcome is unexercised.

| Milestone | Entry / consumed evidence | Required outcome and ceiling |
|---|---|---|
| 06A | Approved LAB-01 scope; existing SDK/fixture inspection | Bounded job/observation/reconciliation contract and host model; host evidence only |
| 06B | 06A and Phase05 real VFS/native-fixture outcome | Native QEMU workflow and restart/readback oracle; modelled plant explicitly labelled, no hardware claim |
| 06C | 06B plus exact carrier/fixture/controller/sensors/metrology/safety package and physical authorization | Actual rack-A to rack-B placement and task-quality/recovery evidence on identified equipment |
| 07A | Accepted 06A common job/handoff contract; approved BASE-01 scope | Stationary eligibility, tray custody and uncertainty contract/host model |
| 07B | 07A, 06A and Phase05 reusable native-fixture evidence | Native QEMU handoff/mode witness; no locomotion or balance claim |
| 07C | 07B plus exact base/controller/stability/protection package and physical authorization | Actual secured-tray transfer between stations under independently qualified motion/safety conditions |
| 08A | Accepted 06A + 07A contracts; approved ASSEMBLY-01 scope | Stationary configuration transition and independent/combined-mode contract/host model |
| 08B | 08A, 06B and 07B | Native QEMU composition with fresh eligibility and reconciliation; no cross-node/hardware claim |
| 08C | 08B, 06C, 07C plus coupling/power/support/safety package and physical authorization | Physical stationary assembly, combined LAB-01 and declared independent configurations |

Physical packages bind dimensions/mass, fixture clearance and numerical acceptance limits, observation/metrology method, exact part/firmware/interface identity, safety owner and safe disposition. Missing values block only their physical milestone; do not invent tolerances or substitute model observations. No prerequisite is satisfied merely because its document exists.

## Execution and Ownership
Phases 01–03 have no semantic ordering dependency. They may overlap only with disjoint file ownership; serialize bench/harness files and shared kernel edits through Main. Default WIP is one native outcome plus one supporting lane. Run shared build/format/lint/suites once after a concurrent edit wave settles, not inside writing agents.
Main owns roadmap/changelog projection and phase integration. Phase02 owns grant/quota/scheduler; Phase03 owns supervisor/demo state semantics; Phase01 owns generic perf collection and runner. Phase04 binds each row to its measured source, rerunning affected rows after fixes. Phase05 owns the private native-test restart bridge and workload, integrating shared supervisor files after Phase03.
Phase06 owns the lab job/oracle extension; Phase07 owns the base handoff contract; Phase08 owns stationary composition. Main serializes any shared bench, image, supervisor or SDK writes. No new generic robot middleware, public robot ABI or unqualified hardware-driver stub is required by this plan.
G1 and G2 may execute concurrently only when explicitly scheduled and ownership/WIP allow it. Shared VFS, IPC, scheduler, app-isolation, image/build and roadmap files are serialization points, not a global product-stage dependency. Evidence does not transfer between lanes: a G1 QEMU/physical PASS does not qualify an office/server workload, and a G2 application PASS does not qualify robot motion or safety.
`260827-1004-hardware-independent-roadmap` Phase 08 shares projection files only; coordinate those writes without adding a false whole-plan dependency. Completed MemInfo, reactor/stack, x86, HDMI and C2C local-oracle lanes remain regression-only.

## Frozen Boundaries
No edits to `libs/api/`, `libs/types/`, syscall IDs, public wire layouts, KMS ABI, Manifest-v3 or production admission. Any necessity for such edits is a separate exact Law-1 design checkpoint, then implementation approval under current code standards; this broad approval is not that package.
Current Build authorization excludes procurement, sensor/camera activation, motor actuation, cloud deployment, remote transport enablement, production keys and hardware qualification. Physical workflow milestones are planned outcomes, not currently authorized runs; each requires its exact package and applicable approvals. Shared-SAS FFI remains trusted, not a sandbox. Preserve quarantine and DMA acknowledgement ordering.

## Evidence and Assumptions
[Scout report](./scout-report.md) records observed experiments, source facts, prior runtime results and precedents. Before each Build, re-ground modified files and verify that source, image and runner are current. Missing QEMU prerequisites must fail/mark BLOCKED, never skip to PASS.
Quota drift and combined-image RedoxFS restart remain runtime-unverified. Phase03 must resolve requester identity, transaction-bound stash and snapshot/quiescence/late-completion fences before Build; frozen-interface gaps require an exact design checkpoint. Phase05 owns a narrow private restart bridge because the current trigger is hypervisor-only. These gates do not block independent baseline rows.
Performance validity and performance target success are separate. The existing `<10 MiB` objective stays unchanged; baseline completion may report TARGET_MISS and must not be called efficiency qualification.
Tier2 has implemented RV64/QEMU substrate and cross-hart migration evidence, limited to internal test-hooks with no public application admission/loader route (see `docs/project-roadmap.md`, Current Codebase Facts, and `kernel/src/loader/domain_admission.rs`). Physical containment, DMA quarantine and production approvals remain open. App-guide projections retain both implementation evidence and the unavailable application route; no qualification is inferred.

## M4 Reopening Events
- Direct dispatch: Phase 04 stage breakdown demonstrates a material bottleneck and a separate ABI/lifetime/identity design is approved.
- Tier 2: existing Spec-22 loader/user-copy/revoke/DMA/hostile/approval gates; never fallback to SAS.
- Remote/public C2C: protected identity/persistence/authenticated-time and authority-owned transport gates from the owning lane; no TCP workaround.
- Production: provisioned anchors, provenance/admission, secure boot, qualified floor, physical evidence, approvals and ledger closure. Host evidence authenticates carriage, not promotion.
- Physical native/robot workload: exact-board and carrier/fixture/controller/sensor identity, metrology and safety package, explicit activation authorization for the owning lane, and all applicable claim gates. Phases06C/07C/08C are externally gated; unrelated host/QEMU work does not wait on them.

## Risk Assessment
Undo source changes by scoped revert before release; preserve prior raw evidence and publication history. Kernel rollback requires compatible rebuilt kernel/cells/image, never mixed grant/lifecycle semantics. No irreversible external operation is authorized. Invalid new evidence cannot become valid merely by reverting a parser.

## Red Team Review
Three parallel reviews completed: 12 reported findings, 10 distinct (8 Major, 2 Minor), all accepted with phase-local corrections or explicit contract gates. See [adjudication](./red-team-review.md). Source review is not exploit/runtime proof; no second reviewer pass or blanket security approval is claimed.
Four parallel reviews of the extension reported seven actionable findings; all were accepted into the plan/docs or explicit implementation acceptance gates. See the separate [review and validation record](./workflow-extension-review.md). Original phase01–05 review findings remain historical evidence, not automatic approval of new robot contracts; source findings are not runtime exploit proof.
A subsequent Red Team pass generated two accepted mitigations: Phase 05 now requires a VFS backend generation-check before disk commit to prevent split-brain during client timeouts, and Phase 06 explicitly limits `ReconcileRequired` clearance to an authenticated out-of-band operator action. One deferred finding regarding bounded extent parsing limits was added to Phase 05 Risk Notes.

## Validation Log
Approach A and the lab-first product/workflow ordering are approved. Phase03 authority/fence design and any exact Law-1 delta remain prerequisites, not implied approval. The new workflow scope is LAB-01, BASE-01 and ASSEMBLY-01; the owner's request authorizes documenting and scheduling them, not inventing physical hardware/precision/safety evidence. No runtime, ABI, ledger, hardware or qualification status is promoted by this amendment.
The owner narrowed organizational replacement to web/app/microservice servers and PCs with common basic office software. ORG-SRV-01/ORG-PC-01 record the functional floor, proposed application references and explicit native/guest/remote claim boundaries; specialized devices are not required. LAB-01 remains the first active product outcome; accepting profile scope does not preapprove application ports or Linux/Windows elimination claims.
Planning validation checks the eight phase templates, source/proposed paths, local links/heading targets and whole-phase/milestone DAGs; `validation.json` records the current result. No runtime tests, physical trials or second reviewer approval are claimed. The discovered VFS reply-binding source gap must be fixed and exercised in Phase05 before 06B consumes authenticated acknowledgement evidence.
Phase01's collector/validity implementation passed its 16 contract tests and retained a structurally valid profile-v2 QEMU diagnostic capture. Review found that its commit and artifact hashes do not bind the dirty benchmark source tree, so Phase01 remains partial, target/history verdicts are diagnostic only, and its exact-evidence dependency edge stays closed pending a source-bound recapture.

## Handoff
`/hc-cook /home/dmin/cellos/.agents/260905-1139-sas-lbi-outcome-closure/plan.md`
Start source execution at Phase01; under the same WIP policy, 06A contract/host work is the first product-facing milestone and may proceed before Phase05. 07A implementation/acceptance waits for the accepted 06A contract; 08A waits for accepted 06A+07A. Drafting can overlap without duplicating shared contracts. 06B waits for Phase05; all B/C milestones follow the table. Do not execute M4 reopening events or 06C/07C/08C as already authorized tasks. Preserve all original M0–M3 outcomes.
