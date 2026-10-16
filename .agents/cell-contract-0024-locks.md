---
lockToken: CELL-CONTRACT-0024
controller: .agents/261011-1200-cell-contract-authority/plan.md
recorded: 2026-10-11
mode: scoped-prerequisite-development
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
---

# CELL-CONTRACT-0024 — related-plan lock inventory

**Recorded:** 2026-10-11. **Controller:** [Cell contract and authority plan](261011-1200-cell-contract-authority/plan.md).
**Direction:** [ADR-0022](../docs/decisions/0022-intel-x86-64-c2c-only-direction.md) remains Intel x86-64 C2C only; [ADR-0024](../docs/decisions/0024-cell-kernel-authority-contract.md) owns the new node-local Cell/Kernel Cell/authority and scoped cluster placement/autoscaling direction.

## Meaning and precedence

This is a scheduling inventory, not a runtime audit or a new completion claim. The inventory covers the 161 existing `.agents/**/plan.md` entrypoints found before creating the controlling plan. Header scopes, literal status fields, relevant phase ledgers and the existing portfolio were reviewed. Stale headline status is retained, not mistaken for absent implementation. The new controlling plan itself is not locked by its own inventory.

There are 38 scoped lock relations below, 34 explicitly related completed/superseded/parked evidence records without a new lock, and 89 other considered exclusions. These are document-inventory counts, **not** counts of unfinished implementations. Existing source, approvals, completed checkboxes and evidence ceilings are not reset.

Each locked entrypoint carries the repository-relative machine-readable relation `blockedBy: [.agents/261011-1200-cell-contract-authority/plan.md]` (alongside any prior blockers), `executionStatus: blocked`, `executionLock: CELL-CONTRACT-0024`, and a specific `lockScope`. Original `status`/progress remains historical; where no status field existed, none was invented. The blocked execution state applies only to the named unfinished/deferred scope, including a scoped future handoff from an otherwise completed record.

**Precedence covers every subordinate phase and direct phase-file link.** Old headers, checkboxes, commands or owner grants cannot bypass this inventory. Original documentation-only admission is superseded **solely** by the user-requested [scoped G1/G2 development authorization](261011-1200-cell-contract-authority/scoped-g1-g2-authorization.md) and its pinned manifest: controlling P01 owns G1 Intel-private edits then G2-R-core/B disposable-image edits, with actual-diff/capture gates. This is no legacy-plan unlock or blanket ABI approval. All38relations remain effective; G2-J, downstream deltas, remote/production, procurement and protected provisioning remain blocked.

The approved red-team architecture amendment covers Spec 25 v1.3 AC-01..AC-50,
explicitly amending affected earlier semantics. Independent fixed-home Budget
Authority enforces issuer-precommitted genesis and durable finite reservations;
controller owns policy/membership and cannot mint allowance. Destination live
claim precedes publication; uncertain children retain charge until key closure.
Challenge freshness, old-generation reconciliation, homogeneous scaling domains,
immutable maxima/current allocations, Destination Ready provenance and origin
single-target proof enter P03/P08 and full P10 qualification. One Cell remains
node-local. Ten phases and all 38 lock relations stay unchanged; no runtime,
ABI, remote or production activation is authorized by documentation.
Single-writer and relay-first/direct-LAN gates remain; no consensus, DSM,
timeout promotion or implicit fallback.

[Spec 26](../docs/specs/26-workload-placement-scaling-contract.md) details P08
workload-router/load-balancing, placement, budget-share and autoscaling ownership.
Its internal slice gates do not create new executable legacy-plan dependencies.
The lock set remains the same 38 relations; no broader unrelated work is frozen.

The 2026-10-14 scoped exception is a controlling-owner prerequisite handoff, not removal of any `blocks`/`blockedBy` relation. Its exact candidate/path/baseline manifest is authoritative for that exception; scope drift requires amendment. No subordinate old plan may claim it as permission to execute.

### Proposed N02-A internal prerequisite disposition — NOT ACTIVE

The user selected preparation of [native development handoff r1](261011-1200-cell-contract-authority/native-development-handoff-r1.md), not implementation authority. Its companion pins exact bytes. N02-A is a proposed controlling-P01-owner exception solely for per-authority arena invariants in `kernel/src/cell/cspace.rs` and its actual-module `tests/unit_cspace_smoke.rs`; exact documentation/output paths are enumerated in that packet. Scheduler/cap/cell consumers remain read-only; WIP is one tranche, before separately gated N02-B→N03→N04→N05→N06/N07. No public/persistent bytes, opcode, SDK, operational authority or production activation changes.

This scheduling disposition is conditional: **only a later explicit approval naming the reviewed packet revision/digest and an appended decision record activates N02-A**. Until then, implementation remains blocked. Even after that scoped approval, all 38 `blocks`/`blockedBy` relations and legacy scopes remain effective; no subordinate plan resumes. Evidence is actual-module host transitions plus compile/link/boot-only native observations, not whole-system bounded fit, Gone/device quiescence, full P02 or downstream CP1/CP2. G1/G2 grants remain unchanged; D05/D07, protected/physical/backend and successor gates are not waived.


Unrelated work is not suspended merely because it mentions a Cell, capability, driver, thread, IPC or Rust. Ordinary consumers and baseline repairs that do not change these contracts have no new lock here; they still need a concrete Intel C2C connection and portfolio admission under ADR-0022. Non-Intel hardware, GUI/browser, AI, robot and general ecosystem programs stay parked under that older direction decision.

## Replacement areas and dependency direction

All adopted slices first pass **P01: exact contract/ABI inventory, approval and adoption**. Replacement areas in the tables name ownership, not permission to execute:

| Area | Controlling phase |
|---|---|
| Cell principal/incarnation, Kernel substrate and shared CSpace | P02 |
| Task/thread identity, SMP linearization and CPU budgets | P03 |
| Correlated operations, wait/completion, cancellation and no automatic post-dispatch replay | P04 |
| Typed resource handles, delegation/attenuation, revocation, DMA/device and namespace authority | P05 |
| Lifecycle, durable logical service state, recovery and crash evidence | P06 |
| Optional POSIX facade and runtime adaptation without ambient UID/root authority | P07 |
| Protected remote identity, exported authority, RPC/lifecycle and scoped placement/autoscaling with durable application-wide budget | P08 |
| Intel VMX/EPT and explicit guest bridge | P09 |
| Physical/profile qualification, actual concurrent local/cluster clients, admission/scale/failure matrix and measured same-hardware Linux comparison | P10 |

The dependency direction is **legacy unfinished scope -> controlling plan**. Completed legacy outputs are evidence inputs; the controlling plan must not wait for execution or completion of a locked legacy plan. Necessary unfinished prerequisites (including kernel repair, async completion, protected-authority work and Intel VMX) are adopted into named internal controlling phases with ownership and retained gates. Older inter-plan `blockedBy`/`blocks` entries remain provenance and cannot form a new circular unlock dependency. A missing completed prerequisite becomes a named internal task or explicit external gate, not an instruction to run the locked predecessor.

## Exact new blocks

For every row: prior status/progress is retained; current scheduling is **BLOCKED within the listed scope**. Existing blockers remain in force. P01 rebase plus explicit owner handoff and the phase-local approvals are required before any selected slice is released.

| Existing plan | Prior status / progress (preserved) | Blocked scope | Replacement area / rebase gate |
|---|---|---|---|
| [260528-2016-vicell-full-implementation](260528-2016-vicell-full-implementation/plan.md) | pending | Only unfinished kernel, Cell, authority, runtime and lifecycle contract slices of the legacy umbrella; unrelated historic product scope stays parked. | P01 then P02/P03/P05/P06; explicit per-slice handoff |
| [260603-1803-phase-h-vfs-hardening](260603-1803-phase-h-vfs-hardening/plan.md) | pending | Unfinished kernel permission and VFS mutation authority contract slices only; unrelated utility breadth not newly frozen. | P01 then P02/P05; explicit per-slice handoff |
| [260605-1406-phase28-wasm-cells-epmp](260605-1406-phase28-wasm-cells-epmp/plan.md) | PARTIAL/SUSPECT (D38 correction, 2026-08-01) | Unfinished WASM host/import authority or tier reinterpretation; RISC-V ePMP remains parked and hardware-gated. | P01 then P02/P03/P05/P07; explicit per-slice handoff |
| [260605-1538-milestone-2-1-vfs-complete](260605-1538-milestone-2-1-vfs-complete/plan.md) | PLANNED headline; P01-P04 DONE; P05 in progress | Remaining Phase05 authority/async-read integration evidence must rebase; completed P01-P04 not reopened. | P01 then P02/P04/P05; explicit per-slice handoff |
| [260605-2107-full-reliability-track](260605-2107-full-reliability-track/plan.md) | planned | Unfinished fault/reclaim/supervisor/restart/state/scheduler contracts, not a second root supervisor program. | P01 then P06; explicit per-slice handoff |
| [260607-1200-phase-27-protocol-hardening](260607-1200-phase-27-protocol-hardening/plan.md) | Planned | Unfinished typed protocol and syscall-authority contract changes; earlier ABI gates retained. | P01 then P02/P04/P05; explicit per-slice handoff |
| [260615-1325-vicell-app-sdk-v1](260615-1325-vicell-app-sdk-v1/plan.md) | Planned | Unfinished ServiceRef, dispatch, AppContext and SDK lifecycle contract work; newer completed SDK behavior is evidence, not to be rebuilt. | P01 then P03/P04/P07; explicit per-slice handoff |
| [260621-0830-cell-perms-p2-p5](260621-0830-cell-perms-p2-p5/plan.md) | P01-P04 done/logic done; delegation/cap_args followups deferred | Deferred syscall delegation, parameterized authority and reopened root/bootstrap policy only; completed P2/P5 and later revocation work retained. | P01 then P02/P05; explicit per-slice handoff |
| [260624-0630-kernel-boundary-cleanup](260624-0630-kernel-boundary-cleanup/plan.md) | pending | Unfinished kernel-versus-Driver-Cell ownership and MMIO/DMA/IRQ/registry authority migration. | P01 then P02/P03/P05/P10; explicit per-slice handoff |
| [260707-1726-g2-loader-redesign](260707-1726-g2-loader-redesign/plan.md) | P01-P06 completed; P07 deferred; P08 in-progress | Deferred loader/address-access and remaining regression closure; P01-P06 completed outputs retained. | P01 then P02/P03/P05; explicit per-slice handoff |
| [260711-1917-tier3b-x86-vtx](260711-1917-tier3b-x86-vtx/plan.md) | pending | Intel VMX/EPT P09 and unfinished guest-lifecycle/authority integration; AMD remains parked. | P01 then P09; explicit per-slice handoff |
| [260712-0800-supervisory-cell-migration](260712-0800-supervisory-cell-migration/plan.md) | complete (phases 00-04 complete; post-plan followups recorded) | Deferred/post-plan snapshot, restart and replacement authority followups only; completed P00-P04 not reopened. | P01 then P06; explicit per-slice handoff |
| [260712-0952-tier3b-vm-hardening-compat](260712-0952-tier3b-vm-hardening-compat/plan.md) | pending | Unfinished shared guest backing, device and lifecycle authority; non-Intel/platform breadth stays parked. | P01 then P05/P09; explicit per-slice handoff |
| [260712-1000-cell-package-distribution](260712-1000-cell-package-distribution/plan.md) | pending | Unfinished installer, cell-store mutation, package/spawn and replacement authority. | P01 then P02/P05; explicit per-slice handoff |
| [260712-1900-manifest-v2](260712-1900-manifest-v2/plan.md) | complete (P00-P02 landed; P03 deferred) | Deferred P03 cap_args and any new manifest authority interpretation only; completed P00-P02 unchanged. | P01 then P02/P05; explicit per-slice handoff |
| [260712-1902-dice-attestation-identity](260712-1902-dice-attestation-identity/plan.md) | queued (P00 complete; P01-P05 pending; P06 deferred) | Pending P01-P05 and deferred P06 when needed for identified Intel C2C identity; protected-root gates retained. | P01 then P02/P05/P08; explicit per-slice handoff |
| [260722-0917-g4-full-std-tier1](260722-0917-g4-full-std-tier1/plan.md) | pending | Unfinished std/PAL, task/thread, owner-scoped I/O and reactor contracts; later completed PAL outputs consumed as evidence. | P01 then P03/P07; explicit per-slice handoff |
| [260722-2330-tier3b-finish-g5-lite](260722-2330-tier3b-finish-g5-lite/plan.md) | pending | Unfinished guest clone, VMID, reset, snapshot and shared device authority; unrelated wide-guest breadth stays parked. | P01 then P06/P09; explicit per-slice handoff |
| [260727-2101-midori-lessons-cellos](260727-2101-midori-lessons-cellos/plan.md) | Completed P01-P11; named boundaries deferred | Deferred init recovery, generic reactor, async VFS/DMA, RecvScatter and permission-reuse followups only; completed P01-P11 unchanged. | P01 then P02/P04/P05/P06; explicit per-slice handoff |
| [260801-midori-phase01-completion](260801-midori-phase01-completion/plan.md) | partial | Blocked conditional ReadGrant and fast-IPC followups only; completed evidence retained. | P01 then P02/P04/P05; explicit per-slice handoff |
| [260801-parallel-midori-closure](260801-parallel-midori-closure/plan.md) | blocked | Unfinished read-grant and completion/peer-death or merge-gate execution only; completed audit/history retained. | P01 then P02/P04/P05; explicit per-slice handoff |
| [260808-1544-wx-cross-hart-tlb-shootdown](260808-1544-wx-cross-hart-tlb-shootdown/plan.md) | blocked | Unfinished or reopened cross-hart permission retirement and reuse gates; non-Intel expansion remains parked. | P01 then P02/P03; explicit per-slice handoff |
| [260809-0922-sas-lbi-revocable-vfs-access](260809-0922-sas-lbi-revocable-vfs-access/plan.md) | in-progress | Unfinished caller migration, pointer retirement, owner-scoped handles and closure gates. | P01 then P02/P05; explicit per-slice handoff |
| [260819-1416-port-common-drivers-g1-g2-g3](260819-1416-port-common-drivers-g1-g2-g3/plan.md) | blocked | Unfinished or reopened shared device/MMIO/DMA authority contracts only; completed P01-P05 retained and G3 expansion remains parked. | P01 then P05/P10; explicit per-slice handoff |
| [260821-0642-app-tiers-completion](260821-0642-app-tiers-completion/plan.md) | in-progress | Unfinished admission, runtime, native-domain, manifest and promotion slices; completed SDK/ledger outputs retained. | P01 then P02/P03/P07/P09/P10; explicit per-slice handoff |
| [260821-1700-tier1-baseline-admission](260821-1700-tier1-baseline-admission/plan.md) | pending | Unfinished publisher, owner A/B, external-floor and production-admission gates. | P01 then P02/P03/P05/P07; explicit per-slice handoff |
| [260821-1800-tier1-rust-std-pal-feasibility](260821-1800-tier1-rust-std-pal-feasibility/plan.md) | FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN APPROVAL BLOCKED | Pending security backing, human approval and PAL owner/lifetime promotion; verified feasibility package retained. | P01 then P02/P03/P05/P07; explicit per-slice handoff |
| [260822-phase08-manifest-predesign](260822-phase08-manifest-predesign/plan.md) | blocked | Future manifest handoff/implementation only; completed non-promotional predesign retained. | P01 then P02/P05; explicit per-slice handoff |
| [260825-1726-kms-silo-production-root](260825-1726-kms-silo-production-root/plan.md) | in-progress | Unfinished mTLS, relay, protected signer and production-root integration; completed NO-GO and frozen-ABI records retained. | P01 then P08/P10; explicit per-slice handoff |
| [260825-sdk-delivery](260825-sdk-delivery/plan.md) | P01 implemented; P02 client blocked; P03-P05 verified; P06 partial | Unfinished Phase02 client/relay authority and Phase06 integration closure; verified unrelated SDK/UI results retained. | P01 then P04/P08; explicit per-slice handoff |
| [260826-1605-phase4-dev-reference-authority](260826-1605-phase4-dev-reference-authority/plan.md) | blocked | Unfinished protected authority, time, enrollment and KMS handoff contracts; no hardware, AWS or production activation. | P01 then P08/P10; explicit per-slice handoff |
| [260827-1004-hardware-independent-roadmap](260827-1004-hardware-independent-roadmap/plan.md) | in-progress | Unfinished shared security, guest-lifecycle, C2C and contract scheduling slices; unrelated platform/desktop work remains parked under ADR-0022. | P01 then P02/P03/P07/P08; explicit per-slice handoff |
| [260827-1015-sas-caller-owned-range-ledger](260827-1015-sas-caller-owned-range-ledger/plan.md) | runtime_evidence_complete_approval_pending | Pending approvals and further owner-generation lease integration; runtime evidence and frozen GetRandom ABI retained. | P01 then P02/P03/P05; explicit per-slice handoff |
| [260905-1139-sas-lbi-outcome-closure](260905-1139-sas-lbi-outcome-closure/plan.md) | in-progress | Unfinished Phase03 authenticated state/replacement transaction and related shared authority work; completed workload evidence retained, robot physical phases remain parked. | P01 then P06; explicit per-slice handoff |
| [260927-0739-kernel-architecture-repair](260927-0739-kernel-architecture-repair/plan.md) | in-progress | All unfinished grant/domain, snapshot and qualification slices; completed repairs remain evidence. | P01 then P02/P03/P05/P06/P10; explicit per-slice handoff |
| [260927-1100-c2c-anywhere-tier-aware](260927-1100-c2c-anywhere-tier-aware/plan.md) | pending | All unfinished local, async, fastpath, LAN, relay and tier-adapter contract execution. | P01 then P04/P08; explicit per-slice handoff |
| [261004-1957-x86-pc-lane](261004-1957-x86-pc-lane/plan.md) | in-progress | Unfinished device-authority or qualification changes affecting Cell contracts; P01-P06 QEMU evidence retained and P07 procurement gate retained. | P01 then P05/P10; explicit per-slice handoff |
| [261006-0934-httpd-single-cell-concurrency](261006-0934-httpd-single-cell-concurrency/plan.md) | pending | All unfinished reactor, async IPC, completion-capacity and connection-lifecycle slices. | P01 then P03/P04/P07; explicit per-slice handoff |

## Related evidence and retired/parked records — no new execution lock

These entries were considered explicitly. Their original status remains unchanged; stale headline wording is explained rather than rewritten. A future contract-changing proposal goes through P01 as new/adopted work; reading or reusing completed evidence needs no unlock. Deferred scope already owned by a locked successor must not reopen through a completed predecessor.

| Existing plan | Prior status / progress -> current disposition | Scope, evidence use or exclusion gate |
|---|---|---|
| [260605-0958-phase24-perf-kaslr](260605-0958-phase24-perf-kaslr/plan.md) | PLANNED headline; both phase rows COMPLETE -> unchanged history/parked; no new lock | Completed benchmark/KASLR history; any selected new CPU-budget or comparison work belongs to P03/P10. |
| [260605-2102-milestone-3-4-micropython-enhancement](260605-2102-milestone-3-4-micropython-enhancement/plan.md) | pending headline; phase rows complete -> unchanged history/parked; no new lock | Completed typed VFS bridge consumer history, not new runtime/authority contracts. |
| [260606-1503-rt-and-service-registry](260606-1503-rt-and-service-registry/plan.md) | No top-level status; registry DONE and RT observability slice DONE -> unchanged history/parked; no new lock | Historical service-ID registry/RT evidence; new endpoint authority, lifecycle and CPU budgets belong to P03/P04/P06. |
| [260607-1420-h-ext-hypervisor-cap](260607-1420-h-ext-hypervisor-cap/plan.md) | No top-level status; both phase rows Done -> unchanged history/parked; no new lock | Completed RV64 hypervisor-cap history; no Intel VMX qualification, no active non-Intel work. |
| [260607-1543-x86-hal-bringup](260607-1543-x86-hal-bringup/plan.md) | In Progress; older bringup table incomplete -> unchanged history/parked; no new lock | Historical platform bringup predecessor; newer x86 IDT/PC evidence is retained. No separate platform program; any future Cell/kernel authority change must enter P01 rather than resume this old table. |
| [260611-0636-net-tools-lookup-service](260611-0636-net-tools-lookup-service/plan.md) | pending headline; sole phase Complete 2026-06-11 -> unchanged history/parked; no new lock | Completed service-lookup consumer migration retained; new generation-bound service semantics belong to P04/P06. |
| [260613-1200-native-fs-srv-redoxfs-nvme](260613-1200-native-fs-srv-redoxfs-nvme/plan.md) | No top-level status; four phase rows Complete -> unchanged history/parked; no new lock | Completed predecessor storage evidence; newer CellosFS/state work is not reopened by this predecessor. |
| [260621-1823-g14-tls-server-auth](260621-1823-g14-tls-server-auth/plan.md) | Done P00-P03; P04 deferred -> unchanged history/parked; no new lock | TLS web-server certificate verification is not remote Cell authorization; deferred external-interoperability evidence stays parked unless named by P08. |
| [260623-1500-tls-server-accept](260623-1500-tls-server-accept/plan.md) | PARKED G2 fallback (explicit section) -> unchanged history/parked; no new lock | External HTTPS server fallback, explicitly not native Cell cluster transport; no new contract freeze or activation. |
| [260624-cell-to-cell-anywhere](260624-cell-to-cell-anywhere/plan.md) | SUPERSEDED 2026-09-27; P00-P03 historical foundation -> unchanged history/parked; no new lock | Unfinished execution already transferred to the now-locked tier-aware plan; no independent unlock route; P04/P08 consume evidence only. |
| [260712-1100-loader-trust-repair](260712-1100-loader-trust-repair/plan.md) | complete, 721e1f6f -> unchanged history/parked; no new lock | Spawn-ceiling/no path-derived cap escalation evidence for P02/P05; historical dependency is satisfied, not a live prerequisite. |
| [260712-1836-mythos-g123-analysis](260712-1836-mythos-g123-analysis/plan.md) | analysis-complete -> unchanged history/parked; no new lock | Trust/spawn/manifest/runtime design dossiers are P01 inputs, not implementation authorization. |
| [260712-1901-cap-revocation](260712-1901-cap-revocation/plan.md) | completed P00-P05, 2026-09-25 -> unchanged history/parked; no new lock | Revocation, grant quarantine, MMIO/DMA teardown evidence for P02/P05; residual physical/x86 claims remain unqualified, not a reopened implementation program. |
| [260712-1903-thread-cellid-quota-fix](260712-1903-thread-cellid-quota-fix/plan.md) | done kernel-side; VFS followup tracked elsewhere -> unchanged history/parked; no new lock | Thread/CellId/quota evidence for P02/P03; deferred VFS owner work uses the newer completed owner-lifetime record or controlling phases. |
| [260731-1653-wx-post-reloc-and-f1-signing](260731-1653-wx-post-reloc-and-f1-signing/plan.md) | completed -> unchanged history/parked; no new lock | Recv buffer-pinning/mailbox correction retained for P04; deferred completion migration is covered by the Midori/C2C locks. |
| [260805-1833-midori-closure-execution](260805-1833-midori-closure-execution/plan.md) | completed -> unchanged history/parked; no new lock | Completed Midori contract/runtime closure is P01/P04/P05 evidence; not an alternate owner. |
| [260806-1026-midori-reactor-stack-closure](260806-1026-midori-reactor-stack-closure/plan.md) | completed -> unchanged history/parked; no new lock | NET_RX, Recv/peer-death, gate and conservative stack evidence for P03/P04; deferred generic reactor is covered by the scoped Midori umbrella lock. |
| [260819-1409-cell-to-cell-anywhere-core](260819-1409-cell-to-cell-anywhere-core/plan.md) | superseded 2026-09-27 -> unchanged history/parked; no new lock | Completed local-only evidence retained; unfinished remote integration already transferred to the now-locked tier-aware plan; no remote-runtime claim. |
| [260820-0638-application-tier-taxonomy](260820-0638-application-tier-taxonomy/plan.md) | completed -> unchanged history/parked; no new lock | Terminology/SDK migration evidence for P01/P07; does not own new Cell semantics. |
| [260822-cellos-vfs-smp-006-owner-lifetime](260822-cellos-vfs-smp-006-owner-lifetime/plan.md) | completed; CELLOS-VFS-SMP-006_CLOSED_VERIFIED_RV64 -> unchanged history/parked; no new lock | CellId/TID lifetime evidence for P02/P03/P05; no cross-architecture or new CSpace claim. |
| [260822-phase07-atomic-publication](260822-phase07-atomic-publication/plan.md) | completed prerequisite; umbrella PHASE07_BLOCKED -> unchanged history/parked; no new lock | Loader publication/cleanup evidence for P02/P03; qualification/promotion remains in the locked umbrella, not reset here. |
| [260823-phase07-rv64-domain-qemu](260823-phase07-rv64-domain-qemu/plan.md) | completed -> unchanged history/parked; no new lock | Native-domain RV64 software evidence for P02/P03; not active RV64 feature development or Intel qualification. |
| [260823-rv64-private-root-execution](260823-rv64-private-root-execution/plan.md) | superseded -> unchanged history/parked; no new lock | Retired private-root precursor; consume later completed domain evidence, not unfinished historical execution. |
| [260902-0708-ipc-aware-completion-wake](260902-0708-ipc-aware-completion-wake/plan.md) | completed -> unchanged history/parked; no new lock | Atomic IPC enqueue/wake evidence for P03/P04; raw-zero NET_RX interruption is not a complete new wait contract. |
| [260902-posix-cwd-path-slice](260902-posix-cwd-path-slice/plan.md) | completed -> unchanged history/parked; no new lock | Existing per-task path semantics retained as P07 migration evidence, not a new Cell namespace decision. |
| [260902-six-ordered-follow-up-lanes](260902-six-ordered-follow-up-lanes/plan.md) | in_progress headline; six phase rows completed -> unchanged history/parked; no new lock | Completed CWD/fstat/rename and authority evidence for P05/P07; stale parallel execution text does not reopen lanes. |
| [260902-x86-per-vector-idt](260902-x86-per-vector-idt/plan.md) | complete -> unchanged history/parked; no new lock | x86 trap/interrupt safety baseline for P02/P03; no unfinished Cell authority scope identified. |
| [260903-pcie-multibus](260903-pcie-multibus/plan.md) | completed -> unchanged history/parked; no new lock | PCIe ownership/BDF/IOMMU evidence for P05/P10; not new device authority or hardware qualification. |
| [260903-x86-e1000-dhcp](260903-x86-e1000-dhcp/plan.md) | completed -> unchanged history/parked; no new lock | Intel QEMU NIC/network evidence for P05/P08/P10. |
| [260906-dual-mode-kernel-evolution](260906-dual-mode-kernel-evolution/plan.md) | in_progress; all four phase rows completed -> unchanged history/parked; no new lock | Completed dual-mode/tier/fastpath/physical claims retained at their recorded ceilings; no unfinished phase is reset. Any new Cell contract work goes to P01-P03/P05, not this historical plan. |
| [260916-1200-tier1-rust-std-pal-implementation](260916-1200-tier1-rust-std-pal-implementation/plan.md) | completed -> unchanged history/parked; no new lock | PAL implementation is P07 evidence; pending admission belongs to the separately locked feasibility/umbrella plans. |
| [260922-1549-cell-native-portability-program](260922-1549-cell-native-portability-program/plan.md) | completed; seven phase closure recorded -> unchanged history/parked; no new lock | TLS/futex/pipe/native admission and reference ports are P03/P07 evidence; no task-centric redesign is silently authorized. |
| [260925-1554-staged-usb-hid-isolation](260925-1554-staged-usb-hid-isolation/plan.md) | No top-level status; phase rows 1-3 complete -> unchanged history/parked; no new lock | Completed RPi3 driver packaging/decoding history; no Intel contract execution and no reopening. |
| [260925-2214-beam-parity-b0-actor-supervisor](260925-2214-beam-parity-b0-actor-supervisor/plan.md) | Active headline; phase rows 1-3 complete; portfolio closed B0 -> unchanged history/parked; no new lock | B0 actor/supervisor evidence for P03/P06; B1/B2 remain parked roadmap triggers, not permission to resume this completed slice. |

## Remaining considered exclusions

Every remaining entrypoint from the original glob appears below. **CLOSED** preserves completed/superseded/cancelled records; **PARKED** means an unrelated platform/product lane remains parked by ADR-0022, not by this new lock; **BASELINE** means a consumer, tool or narrow correctness/build slice has no identified contract-cutover ownership. BASELINE is not automatic execution admission. A status of “no top-level status” means the phase ledger is retained, not that the work is absent. If a proposed slice changes Cell identity, shared authority or lifecycle despite these narrower scopes, classify and adopt that slice in P01 before editing; do not silently widen an exclusion.

| Existing plan | Prior status -> current status | Considered scope / disposition |
|---|---|---|
| [2026-08-15-rpi3-pre-mmu-platform-store](2026-08-15-rpi3-pre-mmu-platform-store/plan.md) | No top-level status; phase ledger retained -> unchanged | RPi3 pre-MMU platform store fix. PARKED; unrelated platform/product scope, no new lock. |
| [2026-08-16-rpi3-el1-drop-fix](2026-08-16-rpi3-el1-drop-fix/plan.md) | pending -> unchanged | Board-rpi3-only boot fix that drops from firmware EL2 to EL1h before kmain while preserving the generic AArch64 EL2 virtualization lane.. PARKED; unrelated platform/product scope, no new lock. |
| [2026-08-16-rpi3-netboot-bootstrap](2026-08-16-rpi3-netboot-bootstrap/plan.md) | cancelled -> unchanged | Bootcode.bin-only SD bootstrap plus Windows-native direct-Ethernet DHCP/TFTP for fast board-rpi3 kernel iteration without OTP.. CLOSED; retained evidence, no new lock. |
| [2026-08-16-rpi3-tge-at-probe](2026-08-16-rpi3-tge-at-probe/plan.md) | pending -> unchanged | One-variable board-rpi3 diagnostic to compare AT S1E0R behavior with HCR_EL2.TGE temporarily cleared inside the existing fault probe.. PARKED; unrelated platform/product scope, no new lock. |
| [2026-08-16-rpi3-uboot-static-tftp](2026-08-16-rpi3-uboot-static-tftp/plan.md) | pending -> unchanged | SD-local U-Boot bootstrap for RPi3B v1.2 with static PC-to-Pi TFTP fetching only the Cellos image.. PARKED; unrelated platform/product scope, no new lock. |
| [2026-08-17-cellos-os-name-uname-arch](2026-08-17-cellos-os-name-uname-arch/plan.md) | completed -> unchanged | Align runtime branding, uname architecture reporting, and kernel package/artifact naming with Cellos.. CLOSED; retained evidence, no new lock. |
| [260603-0717-phase-a-network-tcp-integration](260603-0717-phase-a-network-tcp-integration/plan.md) | pending -> unchanged | Wire TCP CONNECT/SEND/RECV in net service; add loopback integration test. Closes the last major gap before IoT story is credible.. BASELINE; no identified contract owner, no new lock. |
| [260603-1922-phase-c-tcp-server-listen-accept](260603-1922-phase-c-tcp-server-listen-accept/plan.md) | pending -> unchanged | Close the last TCP socket-API gap — wire LISTEN/ACCEPT in the net cell, add nc -l server mode + hostname resolution, prove it with a guest-as-server integration test.. BASELINE; no identified contract owner, no new lock. |
| [260603-2101-phase-d-ipc-length-lua-tcp](260603-2101-phase-d-ipc-length-lua-tcp/plan.md) | pending -> unchanged | Fix net cell stale-byte IPC bug, then expose vnet.* TCP API to Lua.. BASELINE; no identified contract owner, no new lock. |
| [260603-2158-phase-e-udp-dns](260603-2158-phase-e-udp-dns/plan.md) | pending -> unchanged | Add UDP datagram sockets to the net cell and a Lua DNS A-record resolver on top of them.. BASELINE; no identified contract owner, no new lock. |
| [260603-2324-phase-f-lua-scripts-vfs](260603-2324-phase-f-lua-scripts-vfs/plan.md) | pending -> unchanged | Run .lua files from VFS and expose vfs.read/write/append/mkdir to Lua scripts. BASELINE; no identified contract owner, no new lock. |
| [260604-1023-phase-o-dynamic-httpd-while-loop](260604-1023-phase-o-dynamic-httpd-while-loop/plan.md) | pending -> unchanged | Per-request VFS reads in httpd (live content) and a while/do/done shell loop.. BASELINE; no identified contract owner, no new lock. |
| [260604-1512-phase-v-redirect-argv-fix](260604-1512-phase-v-redirect-argv-fix/plan.md) | pending -> unchanged | Wire up >> and < shell redirects (helpers already exist) and fix ARGV stash race with per-task personal keys (kernel-only).. BASELINE; no identified contract owner, no new lock. |
| [260604-2018-phase-x-remaining-work](260604-2018-phase-x-remaining-work/plan.md) | pending -> unchanged | VirtIO DMA fix + 3 shell scripting features + MQTT client cell.. BASELINE; no identified contract owner, no new lock. |
| [260605-0738-x5-mqtt-client](260605-0738-x5-mqtt-client/plan.md) | ready -> unchanged | New /bin/mqtt cell: QoS-0 publish + subscribe over TCP via net IPC.. BASELINE; no identified contract owner, no new lock. |
| [260605-1822-milestone-3-shell-utilities](260605-1822-milestone-3-shell-utilities/plan.md) | pending -> unchanged | Fix pipe/redirect capture, add tab-completion, find/uniq/top/kill, fix cp/mv.. BASELINE; no identified contract owner, no new lock. |
| [260606-repair-release-build](260606-repair-release-build/plan.md) | planned -> unchanged | Repair Release Build + Verification Gate. BASELINE; no identified contract owner, no new lock. |
| [260607-1229-robot-demo-arm](260607-1229-robot-demo-arm/plan.md) | No top-level status; phase ledger retained -> unchanged | Plan: Reference Robot Demo — Sensor → Actuator → MQTT on QEMU ARM. PARKED; unrelated platform/product scope, no new lock. |
| [260607-1600-mmc-subsystem](260607-1600-mmc-subsystem/plan.md) | No top-level status; phase ledger retained -> unchanged | MMC Subsystem — Implementation Plan. PARKED; unrelated platform/product scope, no new lock. |
| [260607-1854-compositor-grant-surfaces](260607-1854-compositor-grant-surfaces/plan.md) | Ready for implementation -> unchanged | Plan: Compositor Grant-Based Surface Redesign. PARKED; unrelated platform/product scope, no new lock. |
| [260607-1854-x86-32-aarch32-hal](260607-1854-x86-32-aarch32-hal/plan.md) | No top-level status; phase ledger retained -> unchanged | Plan: x86_32 và AArch32 HAL — Nano Profile. PARKED; unrelated platform/product scope, no new lock. |
| [260607-2038-viui-toolkit](260607-2038-viui-toolkit/plan.md) | No top-level status; phase ledger retained -> unchanged | ViUI Toolkit — Implementation Plan. PARKED; unrelated platform/product scope, no new lock. |
| [260608-1227-viui-embedded-robot-readiness](260608-1227-viui-embedded-robot-readiness/plan.md) | Superseded — capabilities landed through later ViUI plans (D34, 2026-08-01) -> unchanged | ViUI Embedded/Robot Readiness — Implementation Plan. CLOSED; retained evidence, no new lock. |
| [260608-1451-viui-next-phases](260608-1451-viui-next-phases/plan.md) | Superseded by `260616-0755-viui-completion` (D34, 2026-08-01) -> unchanged | ViUI Next Phases — G1 Completion + G2 Roadmap. CLOSED; retained evidence, no new lock. |
| [260608-1500-viui-core-signal](260608-1500-viui-core-signal/plan.md) | No top-level status; phase ledger retained -> unchanged | ViUI v2 P01 — viui-core: Signal Engine + ViRenderer. PARKED; unrelated platform/product scope, no new lock. |
| [260608-1600-viui-widgets-p02](260608-1600-viui-widgets-p02/plan.md) | No top-level status; phase ledger retained -> unchanged | ViUI v2 P02 — viui-widgets: Typed Widget Structs + Macros. PARKED; unrelated platform/product scope, no new lock. |
| [260608-1700-vi-compiler-p03](260608-1700-vi-compiler-p03/plan.md) | No top-level status; phase ledger retained -> unchanged | ViUI v2 P03 — vi-compiler: Lexer + Parser → AST. BASELINE; no identified contract owner, no new lock. |
| [260608-1800-vi-compiler-p04-codegen](260608-1800-vi-compiler-p04-codegen/plan.md) | No top-level status; phase ledger retained -> unchanged | ViUI v2 P04 — vi-compiler: Expression Evaluator + Rust Codegen. BASELINE; no identified contract owner, no new lock. |
| [260608-1900-viui-build-p05](260608-1900-viui-build-p05/plan.md) | No top-level status; phase ledger retained -> unchanged | ViUI v2 P05 — viui-build: Cargo build.rs Integration. PARKED; unrelated platform/product scope, no new lock. |
| [260608-2100-viui-macros-p06](260608-2100-viui-macros-p06/plan.md) | **COMPLETE** — 2026-06-08 -> unchanged | ViUI v2 P06 — vi_design! Proc Macro. PARKED; unrelated platform/product scope, no new lock. |
| [260608-2300-viui-gpu-p07](260608-2300-viui-gpu-p07/plan.md) | COMPLETE — 2026-06-08 -> unchanged | ViUI v2 P07 — GPU Command Buffer Renderer. CLOSED; retained evidence, no new lock. |
| [260608-2355-viui-perf-p08](260608-2355-viui-perf-p08/plan.md) | No top-level status; phase ledger retained -> unchanged | P08 — ViUI v2 Performance: Signal→DirtyRect + Incremental Layout + DrawText Dedup. PARKED; unrelated platform/product scope, no new lock. |
| [260608-2359-viui-perf-p09](260608-2359-viui-perf-p09/plan.md) | No top-level status; phase ledger retained -> unchanged | P09 — ViUI v2 Performance: Close the Slint Embedded Gap. PARKED; unrelated platform/product scope, no new lock. |
| [260609-0000-viui-perf-p10a](260609-0000-viui-perf-p10a/plan.md) | No top-level status; phase ledger retained -> unchanged | P10a — ViUI v2 Performance: 3 Quick-Win Micro-Optimisations. PARKED; unrelated platform/product scope, no new lock. |
| [260609-0030-viui-perf-p10b](260609-0030-viui-perf-p10b/plan.md) | No top-level status; phase ledger retained -> unchanged | P10b — ViUI v2 Performance: 2 Medium-Effort Optimisations. PARKED; unrelated platform/product scope, no new lock. |
| [260609-0100-viui-perf-p10c](260609-0100-viui-perf-p10c/plan.md) | No top-level status; phase ledger retained -> unchanged | P10c — ViUI v2 Performance: Inline blend paths in fill_rect and draw_image. PARKED; unrelated platform/product scope, no new lock. |
| [260609-0601-viui-g2](260609-0601-viui-g2/plan.md) | Superseded/closed — P01-P04 landed; GPU deferred (D34, 2026-08-01) -> unchanged | ViUI G2 — Reactive DSL · FlexBox · Virtual List · Accessibility · GPU. CLOSED; retained evidence, no new lock. |
| [260613-1500-rknn-inference-g2a](260613-1500-rknn-inference-g2a/plan.md) | PLANNED -> unchanged | RKNN Inference Cell — G2 Level A. PARKED; unrelated platform/product scope, no new lock. |
| [260615-1458-gpio-mmio-el0-fix](260615-1458-gpio-mmio-el0-fix/plan.md) | In Progress -> unchanged | GPIO MMIO EL0 Permission Fix — hoàn thiện peripheral suite. PARKED; unrelated platform/product scope, no new lock. |
| [260616-0755-viui-completion](260616-0755-viui-completion/plan.md) | Complete — closed implementation record (D34, 2026-08-01) -> unchanged | ViUI v2 Library Completion Record. CLOSED; retained evidence, no new lock. |
| [260621-1433-hypha-ai-agent](260621-1433-hypha-ai-agent/plan.md) | No top-level status; phase ledger retained -> unchanged | Hypha — ViCell's First Real Application. PARKED; unrelated platform/product scope, no new lock. |
| [260621-1823-ostd-http-json](260621-1823-ostd-http-json/plan.md) | No top-level status; phase ledger retained -> unchanged | Plan — `ostd::http` + `ostd::json` (via `libs/http-core`). BASELINE; no identified contract owner, no new lock. |
| [260712-0900-spi-peripheral](260712-0900-spi-peripheral/plan.md) | pending -> unchanged | SPI is already shipped (Mode-0, TX-only-verified). This plan closes the two real gaps: configurable Mode 0-3 and a software MOSI→MISO loopback so the full-duplex RX path is finally test-covered.. PARKED; unrelated platform/product scope, no new lock. |
| [260712-0901-suite-green-3arch](260712-0901-suite-green-3arch/plan.md) | done (2026-07-13) -> unchanged | Regenerate stale disk images, build a per-arch truth table, and root-cause/fix the input event-delivery reds and the console char-8 stall — debug/verify track.. CLOSED; retained evidence, no new lock. |
| [260712-1001-shell-on-screen](260712-1001-shell-on-screen/plan.md) | pending -> unchanged | Evolve fb-console into a VT100 terminal cell rendering the shell session on HDMI via ViUI + compositor Grant surfaces.. PARKED; unrelated platform/product scope, no new lock. |
| [260724-1632-tier3b-virtio-gpu](260724-1632-tier3b-virtio-gpu/plan.md) | partial -> unchanged | 2D virtio-gpu device model in the ARM64 hypervisor Cell; guest framebuffer copied into a Grant and driven through the existing compositor path.. PARKED; unrelated platform/product scope, no new lock. |
| [260726-full-utility-suite](260726-full-utility-suite/plan.md) | complete -> unchanged | Pipeline-safe grep/sed/mini-AWK plus observable top metrics, with bounded ERE-lite matching and a backward-compatible process telemetry ABI.. CLOSED; retained evidence, no new lock. |
| [260731-1804-dtb-memory-and-runtime-gates](260731-1804-dtb-memory-and-runtime-gates/plan.md) | completed -> unchanged | Replace the RV64 190 MiB fallback with a reservation-safe DTB map, then close the remaining phase 09/11 runtime evidence.. CLOSED; retained evidence, no new lock. |
| [260731-1930-capacity-observability](260731-1930-capacity-observability/plan.md) | completed -> unchanged | Expose cell-spawn OOM faithfully and replace the synthetic memory benchmark with allocator-backed MemInfo telemetry.. CLOSED; retained evidence, no new lock. |
| [260801-d12-hardware-supplement-ruling](260801-d12-hardware-supplement-ruling/plan.md) | Complete -> unchanged | D12 hardware supplement ruling. CLOSED; retained evidence, no new lock. |
| [260801-d13-tier1-signature-admission-ruling](260801-d13-tier1-signature-admission-ruling/plan.md) | Complete -> unchanged | D13 Tier-1 signature-admission ruling. CLOSED; retained evidence, no new lock. |
| [260801-d15-d17-rulings](260801-d15-d17-rulings/plan.md) | completed -> unchanged | Apply approved D15, D16, and D17 documentation/comment corrections without runtime or ABI changes.. CLOSED; retained evidence, no new lock. |
| [260801-d18-d25-rulings](260801-d18-d25-rulings/plan.md) | complete -> unchanged | Apply recommendation A for D18-D25 as documentation/docket updates plus one dead ostd wrapper deletion.. CLOSED; retained evidence, no new lock. |
| [260801-d1b-d3-d5-closure](260801-d1b-d3-d5-closure/plan.md) | complete -> unchanged | Plan to close D1 consequents, D1b IPC target semantics, D3 kernel LOC ownership, and D5 cell-scale profile.. CLOSED; retained evidence, no new lock. |
| [260801-d26-d33-rulings](260801-d26-d33-rulings/plan.md) | Complete — implementation, tests, and review verified -> unchanged | D26-D33 documentation reconciliation. CLOSED; retained evidence, no new lock. |
| [260801-d34-d39-plan-portfolio](260801-d34-d39-plan-portfolio/plan.md) | Complete — validation and independent review passed -> unchanged | D34-D39 plan portfolio reconciliation. CLOSED; retained evidence, no new lock. |
| [260817-0820-rpi3-input-fallback](260817-0820-rpi3-input-fallback/plan.md) | Complete -> unchanged | RPi3 input fallback fix. CLOSED; retained evidence, no new lock. |
| [260817-1437-rpi3-mmc-pinmux](260817-1437-rpi3-mmc-pinmux/plan.md) | completed -> unchanged | Narrow board-rpi3-only plan to route external SD pins to Arasan before existing MMC probe.. CLOSED; retained evidence, no new lock. |
| [260817-2207-cellos-board-soc-hal-split](260817-2207-cellos-board-soc-hal-split/plan.md) | completed -> unchanged | Introduce declarative board packages and route QEMU RV64 boot/platform data through the first descriptor consumer.. CLOSED; retained evidence, no new lock. |
| [260817-hal-soc-bcm27xx-slice](260817-hal-soc-bcm27xx-slice/plan.md) | superseded -> unchanged | Introduce the first real hal/soc crate and move BCM27xx platform/MMC facts out of generic kernel drivers.. CLOSED; retained evidence, no new lock. |
| [260818-0513-hal-soc-riscv-profiles](260818-0513-hal-soc-riscv-profiles/plan.md) | completed -> unchanged | Extract RISC-V SoC profile facts into hal/soc without duplicating shared drivers or changing board descriptors.. CLOSED; retained evidence, no new lock. |
| [260818-0650-riscv-plic-policy](260818-0650-riscv-plic-policy/plan.md) | completed -> unchanged | Move RV64 PLIC IRQ/context policy out of shared mechanism and into configured runtime data.. CLOSED; retained evidence, no new lock. |
| [260818-0845-bcm27xx-soc-facts](260818-0845-bcm27xx-soc-facts/plan.md) | completed -> unchanged | Add a data-only BCM27xx SoC crate and consume existing BCM2837 controller facts without moving board wiring or drivers.. CLOSED; retained evidence, no new lock. |
| [260818-0935-rpi3-board-descriptor](260818-0935-rpi3-board-descriptor/plan.md) | completed -> unchanged | Add an audited Raspberry Pi 3 board descriptor and consume it for platform defaults and fallback memory.. CLOSED; retained evidence, no new lock. |
| [260818-1100-bcm27xx-mmio-policy](260818-1100-bcm27xx-mmio-policy/plan.md) | completed -> unchanged | Move duplicated BCM2837 MMIO spans and grant-window sizes into the data-only SoC profile.. CLOSED; retained evidence, no new lock. |
| [260818-1230-bcm27xx-arch-bases](260818-1230-bcm27xx-arch-bases/plan.md) | completed -> unchanged | Source BCM2837 controller bases from the SoC profile while retaining mechanisms in ARM arch HAL.. CLOSED; retained evidence, no new lock. |
| [260818-1400-bcm27xx-irq-topology](260818-1400-bcm27xx-irq-topology/plan.md) | completed -> unchanged | Move immutable BCM2837 IRQ numbers and local-source masks into SoC data.. CLOSED; retained evidence, no new lock. |
| [260818-1530-bcm27xx-irq-consumers](260818-1530-bcm27xx-irq-consumers/plan.md) | completed -> unchanged | Route remaining BCM IRQ consumers through the centralized SoC topology.. CLOSED; retained evidence, no new lock. |
| [260818-1600-x86-hal-split](260818-1600-x86-hal-split/plan.md) | completed -> unchanged | Add the generic ACPI PC board descriptor and move static x86 platform facts into hal/soc/x86 without weakening firmware gates.. CLOSED; retained evidence, no new lock. |
| [260818-1700-rpi3-uart-debug-reuse](260818-1700-rpi3-uart-debug-reuse/plan.md) | completed -> unchanged | Remove the kernel TrapFrame probe's private mini-UART write implementation.. CLOSED; retained evidence, no new lock. |
| [260818-1756-qemu-q35-x86-board-taxonomy](260818-1756-qemu-q35-x86-board-taxonomy/plan.md) | partial -> unchanged | Rename the current x86 board contract to QEMU q35 scope and add documentation-only 32-bit QEMU placeholders.. BASELINE; no identified contract owner, no new lock. |
| [260818-1900-complete-hal-split](260818-1900-complete-hal-split/plan.md) | completed -> unchanged | Finish descriptor, SoC-data, shared-driver, and build-selection boundaries for current boards.. CLOSED; retained evidence, no new lock. |
| [260819-0623-rename-qemu-x86-32-board](260819-0623-rename-qemu-x86-32-board/plan.md) | complete -> unchanged | Rename the QEMU 32-bit x86 placeholder from q35-i686 to q35-x86_32 without adding runtime support.. CLOSED; retained evidence, no new lock. |
| [260819-2042-codebase-docs-sync](260819-2042-codebase-docs-sync/plan.md) | completed -> unchanged | Docs-only plan to re-baseline Cellos documentation against current source and split the oversized roadmap into stable lookup files.. CLOSED; retained evidence, no new lock. |
| [260819-hal-kernel-abi-single-source](260819-hal-kernel-abi-single-source/plan.md) | completed -> unchanged | Close HAL-to-kernel Rust ABI signature drift with one shared declaration surface and verified compile gates.. CLOSED; retained evidence, no new lock. |
| [260823-rpi3-hardware-completion](260823-rpi3-hardware-completion/plan.md) | in-progress -> unchanged | Five RPi3 enhancement phases: semihosting fix, HDMI framebuffer, persistent SD storage, G1 peripheral drivers, and LAN9514 Ethernet.. PARKED; unrelated platform/product scope, no new lock. |
| [260826-0918](260826-0918/plan.md) | completed -> unchanged | Harden Hypha JSON Parser. CLOSED; retained evidence, no new lock. |
| [260827-0948-desktop-compositor-viui-managed-surface](260827-0948-desktop-compositor-viui-managed-surface/plan.md) | Complete at the RV64 QEMU software-evidence ceiling -> unchanged | Desktop compositor + ViUI managed surface. CLOSED; retained evidence, no new lock. |
| [260831-http-smoke-hard-gate](260831-http-smoke-hard-gate/plan.md) | completed -> unchanged | HTTP Smoke Hard Gate. CLOSED; retained evidence, no new lock. |
| [260902-solo-maintenance-slices](260902-solo-maintenance-slices/plan.md) | completed -> unchanged | Three bounded CellOS slices for truthful memory reporting and removal of two dead kernel APIs.. CLOSED; retained evidence, no new lock. |
| [260913-2002-g2-level-a-ai-inference](260913-2002-g2-level-a-ai-inference/plan.md) | completed (phases 01-07; CP-2/CP-4/CP-5/CP-6/CP-7 remain gated) -> unchanged | G2 Level A — Native AI Inference Service Implementation Plan. CLOSED; retained evidence, no new lock. |
| [260914-ai-oracle-arm64](260914-ai-oracle-arm64/plan.md) | No top-level status; phase ledger retained -> unchanged | Plan — AI inference oracle on the ARM64 leg. PARKED; unrelated platform/product scope, no new lock. |
| [260914-ci-gate-restoration](260914-ci-gate-restoration/plan.md) | No top-level status; phase ledger retained -> unchanged | Plan — Restore the hosted CI gates. BASELINE; no identified contract owner, no new lock. |
| [260914-cpu-engine-optimization](260914-cpu-engine-optimization/plan.md) | No top-level status; phase ledger retained -> unchanged | Plan — CPU inference engine throughput. PARKED; unrelated platform/product scope, no new lock. |
| [260914-q8-integer-activations](260914-q8-integer-activations/plan.md) | No top-level status; phase ledger retained -> unchanged | Plan — Integer Q8_0 activation path. PARKED; unrelated platform/product scope, no new lock. |
| [261007-1904-serval-wpe-feasibility](261007-1904-serval-wpe-feasibility/plan.md) | cancelled -> rejected | Kế hoạch khả thi port WPE WebKit vào Tier 2 bị hủy/bác bỏ hoàn toàn (no-go, 2026-10-16). CellOS chỉ có duy nhất 1 trình duyệt native là Ocel; trình duyệt full chạy Tier 3 per ADR-0017. CLOSED; no new lock. |
| [desktop-decoration](desktop-decoration/plan.md) | complete -> unchanged | Compositor-rendered focused-surface decoration. CLOSED; retained evidence, no new lock. |
| [desktop-window-policy](desktop-window-policy/plan.md) | verified -> unchanged | Two-Surface Raise and Keyboard-Focus Policy. CLOSED; retained evidence, no new lock. |
| [window-manager-policy](window-manager-policy/plan.md) | pending -> unchanged | Compositor titlebars and controls with capability-safe configure, close, and visibility lifecycles.. PARKED; unrelated platform/product scope, no new lock. |

## Unfreeze procedure — no automatic release

1. Identify one exact unfinished slice, the Intel C2C outcome it serves and its controlling phase. Reconcile old status against existing evidence without erasing completed history or assuming unchecked boxes mean missing code.
2. Record the contract delta against ADR-0024 and the controlling contract: Cell principal/incarnation distinct from Task; shared per-Cell capability space; explicit delegation and attenuation; revocation generations; Kernel substrate distinct from ordinary policy Cells; durable logical state distinct from execution. Include the caller/test/doc migration and concurrency/remote impact. No ambient UID/root credential or remote pointer shortcut is admitted.
3. The accountable owner explicitly adopts/rebases that slice into the controlling plan, names its file-owner handoff and confirms no overlapping owner. Adopt unfinished prerequisites internally; consume completed predecessors only as evidence. All unselected legacy scope stays blocked/parked.
4. Record exact retained approval/evidence gates: two Law-1 checkpoints for concrete ABI changes; no-ABI limits where applicable; protected identity/nonrollback time/state; independent human decisions; exact physical/device and production gates. Documentation, a model review, software harness or a green old test cannot substitute for these gates. Direction approval is not ABI approval.
5. In one scheduling change, record the selected slice and acceptance ceiling in the controlling plan and portfolio, narrow/remove only its applicable `lockScope`/`blockedBy` relation, and update this inventory with the named owner, rebase decision and remaining locked scope. Keep the whole-plan relation if other affected scope remains. WIP is one implementation slice, not one slice per child.
6. Completion of ADR/spec/plan documentation, or even completion of the controlling plan, does **not** automatically unlock any old plan. Remote/production activation requires its separate explicit authority and evidence. Urgent in-scope baseline/security repairs are scheduled as named, contract-preserving slices; they are not blanket exceptions to the lock.

## Evidence boundary and handoff

This change edits planning entrypoints and scheduling documents only. No runtime or ABI surface is changed, no tests/build/lint/format are run, and no historical evidence is upgraded. The main integration owner should validate new relative links, machine-readable lock edges/scope, direct-phase precedence and preservation of prior blockers/status. The exact changed plan entrypoints are the 38 rows in “Exact new blocks”; central changes are this inventory, `plan-portfolio.md` and `../docs/roadmap/current-focus.md`.
