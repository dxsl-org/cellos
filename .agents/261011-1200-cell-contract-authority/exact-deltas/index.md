# Exact-delta package set — preparation with scoped semantic successors

Recorded 2026-10-13. Scope: D01–D12 and C01–C11 under [P01](../phase-01-contract-gates.md), [Spec 25 v1.3](../../../docs/specs/25-cell-kernel-authority-contract.md) and [Spec 26 v1.1](../../../docs/specs/26-workload-placement-scaling-contract.md). These are review preparation packages, not implemented interfaces or approved exact CP1 candidates. All ten phases remain pending, all 38 scoped locks remain effective, both checkpoints remain pending, and release remains blocked.

## Baseline and byte identity

Observed Git HEAD: `57a2bbdb0606461ce442ef1c67d02208053a9d3f`; committed tree: `54f7dcc6e2fd96bc1b42fa0ccd83db5bc5f98591`. This does not assert a clean working tree. [The manifest](manifest.json) separately pins actual working-source bytes and each preparation document's SHA-256. It covers the selected source denominator, not ignored archives, external SDKs, expanded macros or a resolved semantic call graph. A digest authenticates preparation bytes only; it is not consent or runtime proof.

Each package's preparation revision is separate from the **complete candidate revision/digest** in [the approval ledger](../abi-deltas.md#per-delta-approval-records). Those exact-candidate fields remain `pending` because measured finite parameters, trust/persistence inputs or complete migration are not closed. D01/D03/D04/D05/D06/D07/D08 retain reviewed r2 preparations; C03 is now r2 and C05 r3 under the [scoped user contract approval](../durable-charge-user-approval.json). That record pins approved presented contracts and retains full predecessor C03 r1/C05 r2 bytes. The manifest pins current package bytes; no checkpoint or release field is promoted. Other packages remain r1. Earlier carrier-series anchors retain their historical meaning; current explicitly revised constraints supersede earlier draft wording. Changing package bytes requires a governed preparation revision and updated manifest, never inferred CP1/CP2 consent.

## Package routing

Each document records current contracts, proposed changes, affected publisher/parser/consumer and removal responsibilities, errors/output/lifetime boundaries, bounded charge requirements, failure scenarios and exact blocking inputs. Table gates are shared prerequisites; package-specific detail lives only in its `Blocking inputs` section. No source-observed declaration is relabeled an exercised consumer, and future service paths are target owners rather than existing implementations.

D01–D07 select concrete **unapproved** local carriers, status/output rules and scheduler-lock linearization rather than deferring those design choices. Examples are D01's private u64 tickets and32-byte status header, D02's40-byte principal, D04's48-byte submit/scoped-wait records, D05's issuer receipt transaction, D06's64-byte use request and D07's lifecycle/evidence records. These layouts are design proposals, not emitted ABI bytes, assigned syscalls or measured capacities. D08–D12/C01–C11 also select semantic cutovers and state ordering; trust/backend-dependent exact fields remain explicitly blocked.

| Package | Integrating owner | Boundary | Shared blocking inputs |
|---|---|---|---|
| [D01](D01.md) | P02 | Local typed/generational handles | B01, B02, B03, B08 |
| [D02](D02.md) | P02/P03/P06/P08 | Principal, Kernel Cell and binding identity | B01, B02, B05, B08 |
| [D03](D03.md) | P02/P03/P05 | Shared CSpace, derivation, close/revoke | B01, B02, B03, B08 |
| [D04](D04.md) | P04 | Operations, reply tokens, wait/consume | B01, B02, B03, B08 |
| [D05](D05.md) | P02/P05/P06 | Service-issued handles and post-start receipt | B01, B02, B03, B04, B08 |
| [D06](D06.md) | P05/P04/P08/P09 | Grant, mapping and device quiescence | B01, B02, B03, B08 |
| [D07](D07.md) | P03/P06 | Thread/Cell retirement and fault records | B01, B02, B03, B06, B08 |
| [D08](D08.md) | P05/P07 | Native stream mechanism | B01, B02, B03, B04, B08 |
| [D09](D09.md) | P07/P02/P05 | POSIX providers, native profiles and bootstrap | B01, B03, B04, B08 |
| [D10](D10.md) | P05/P04/P02 | Ring negotiation, correlation and authority | B01, B02, B03, B08 |
| [D11](D11.md) | P02/P03/P07/P08 | Manifest/syscall declarations and ceilings | B01, B02, B03, B04, B08 |
| [D12](D12.md) | P08 | Local/remote endpoints and cluster carriers | B01, B03, B05, B07, B08 |
| [C01](C01.md) | P08-C/P08-B | Application/workload policy and authority | B01, B03, B05, B07, B08 |
| [C02](C02.md) | P03/P08-C | Budget vectors and immutable maxima | B01, B03, B05, B07, B08 |
| [C03](C03.md) | P08-C/P06 | Genesis and one-use bootstrap | B01, B03, B05, B06, B07, B08 |
| [C04](C04.md) | P08-C/P06 | Reserve, live claim, key closure and recovery | B01, B03, B05, B06, B07, B08 |
| [C05](C05.md) | P03/P08-C/P06 | Growth, pending shrink and drain | B01, B03, B05, B06, B07, B08 |
| [C06](C06.md) | P08-B/P08-C/P06 | Concrete/pool endpoints and Ready provenance | B01, B03, B05, B07, B08 |
| [C07](C07.md) | P04/P08-A/P08-B | Immutable origin and issuer-sealed single target | B01, B03, B05, B07, B08 |
| [C08](C08.md) | P08-B/P08-C/P08-D | Receiver-start challenge freshness | B01, B03, B05, B07, B08 |
| [C09](C09.md) | P08-B | Weighted routing, bounded queues and shares | B01, B03, B05, B07, B08 |
| [C10](C10.md) | P08-D | Generation-complete telemetry and scaling | B01, B03, B05, B07, B08 |
| [C11](C11.md) | P08-C/P08-A | Placement and transport-session boundaries | B01, B03, B05, B07, B08 |

## Blocking-input register

| Gate | Required input and accountable owner | What it prevents |
|---|---|---|
| B01 Reference closure | Implementing owner: resolved references, expanded macros/reexports, cfg/target consumers, numeric C/Zig aliases, actual provider/linker/archive/image and external SDK denominator. Current `CapId` LSP references attempt exited unexpectedly with code 0; lexical inventory remains a fallback. | Exhaustive candidate-specific caller/removal closure; lexical counts cannot authorize a cutover. |
| B02 Local proposal reconciliation and consent | P02/P03/P04/P05/P06 and maintainer: reconcile the selected D01–D07 handle/principal/token/record/status/lock/error proposals across consumers, assign reviewed new entry points and decide the governed incompatible-image release without consuming existing frozen reservations. | Implementation/complete CP1 candidate. Local design is selected, but record bytes do not supply measured profiles, exhaustive migration or consent. |
| B03 Measured finite profile | P01/P03/P04/P05/P08/P10: disjoint charge/physical high-water and all-path capacity; actual distinct-CPU aggregate charging, period/burst/priority/overshoot; saturated operation/session/retention/control overhead. Authorized issuer selects finite limits from that basis. | Numerical budget/profile and bounded-storage qualification. Boot/parked/QEMU snapshots cannot choose safe production values. |
| B04 Compatibility/provider decision | P02/P05/P07 and maintainer: Spec 15/17/22/23 and VFS semantic amendments, version/release decision, native stream/BootFS replacement, exact C/mlibc/Zig provider and generated artifact tuple. | ABI removal, native-only linkage and manifest/profile promotion. No permanent shim or deletion of the VFS bootstrap prerequisite. |
| B05 Authentic issuers and protected proof | P02/P04/P06/P08 plus KMS/Silo owners: assigned policy/budget/Ready/origin/node-boot issuers, approved keys/proof constructors/source sets and qualified protected time where claimed; original Build/AC-012 gates. | Trust-bearing encodings and production remote admission; fixture constructors are not authority. |
| B06 Durable persistence/fencing | P06/P08-C plus authorized issuer/storage owner: admitted backend, exact durable order/flush/error/rollback/key/fence/retention contract, genesis and recovery participants. | Exact persistent bytes and crash-safe reserve/claim/closure/resize; timeout does not release unknown charge. |
| B07 Cluster schema and policy parameters | P08-A/B/C/D: canonical payload/digest/schema/effect scope, origin attenuation, Ready/challenge source sets, finite route/share/queue/window/domain/placement arithmetic and failure outputs, backed by B03/B05/B06 where applicable. | Exact cluster public/persistent candidate; current V1 broker envelope is only a source seed. |
| B08 Governance and implementation evidence | Accountable maintainer plus per-package phase: CP1 on exact complete candidate before locked edits; actual implementation/scenario/source/artifact/profile evidence and CP2 before freeze/release. | Implementation authority and promotion. Architecture approval, preparation digests or document validation supply neither checkpoint. |

### Measured-profile and issuer/persistence candidate inputs

The [measured-profile input package](../measured-profile-candidate.md) separates authenticated capture observations from admissible finite-limit selection. The [issuer/persistence contract package](../issuer-persistence-candidate.md) supplies role separation, proof and durable transition requirements, with backend/trust qualification kept explicit. These prerequisites supplement the 23 pinned preparation packages; they do not silently revise their bytes or manufacture issuer consent.

A current replay of the five descriptor-tree capture matrices with `p01_budget_readiness.py --require-budget-ready` exits2: complete requested matrix,15 blocker classes, `budget_selection_ready=false`, `candidate_budgets=null`, capacity basis `REFUSED`. The replay is preserved in `build/cp1-prerequisites/profile-readiness.json`. B03/B05/B06 remain open until actual qualification and authorized trust/backend inputs satisfy these contracts. Other B01/B02/B04/B07/B08 gates remain independent; supplying these two documents alone cannot close CP1 candidate readiness.

## Review and handoff sequence

1. Resolve the package's `Blocking inputs` with actual named artifacts/decisions, not placeholder/default values. Preserve the approved architectural semantics and current enforcement.
2. Produce exact old/new API/byte/layout/version tables and finite parameter basis, with candidate-specific semantic/build/provider closure. Cross-package consumers must use one selected representation rather than independently minting carriers.
3. Pin a new **exact candidate** revision and digest in the existing per-delta approval ledger; ask the accountable maintainer for CP1 explicitly naming it. Preparation revisions are not that candidate.
4. Implement only through the owning phase and scoped-file handoff; migrate every affected consumer and remove obsolete execution paths in the selected cutover. No interface change or activation is authorized by this package set.
5. Supply failure/race/boundary and applicable hardware/protected/runtime evidence; obtain CP2 on the actual implemented delta. Only then may the governed release/phase state change.

Rollback/rejection: declining any proposed r2 contract leaves the current runtime and frozen ABI enforcement unchanged; no target implementation, irreversible state or messages were produced. Retain initial preparation/validation history and supersede the affected document explicitly rather than erasing it. This is not a runtime migration rollback protocol.

## Reproducible consistency checks

From the repository root:

```sh
python3 scripts/verify-cell-contract-p01.py --negative-controls
python3 scripts/verify-cell-contract-p01.py --require-approved --negative-controls
```

The first checks document/lock/AC consistency, all 23 preparation digests, pinned working-source bytes and rejecting controls. The second must refuse pending approvals with `APPROVAL_PENDING`; any other error remains a defect to resolve. Neither checks new-contract runtime behavior or completes P01. If current source legitimately changes, re-collect the lexical census separately and revise the affected preparation/source pins; do not regenerate hashes merely to conceal drift.
