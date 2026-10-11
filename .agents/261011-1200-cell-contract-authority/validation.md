# Validation — Cell contract planning package

**Date:** 2026-10-11
**Scope:** architecture/plan/lock documents only. No runtime code or ABI changed by this planning task. Implementation criteria AC-01..AC-43 remain pending; documentation consistency is not implementation evidence. AC-30..AC-35 add cluster placement/accounting; AC-36..AC-43 and Spec 26 settle complete workload routing/load balancing/autoscaling. Earlier verification sections are historical baselines, not current runtime proof.

## Decisions checked during recon

- Same-Cell worker reply tokens retained; no invented per-thread security boundary. Kernel-attested provider incarnation and single terminal still required.
- Whole-Cell retirement chosen for unhandled shared-memory-threatening worker faults; explicitly a target semantic amendment, not current task-local behavior.
- Native kernel stream enforcement retained when justified; POSIX fd/personality policy moves outside core.
- VFS post-start receipt explicitly amends spawn-only policy; no duplicate kernel semantic ACL.
- Remote mutable authority fails closed under partition; revoke cutoff/quiescence and already-dispatched uncertainty are separate.
- Existing Law-1, protected authority, Intel physical/VMX and production gates retained. User architecture approval is not unspecified ABI approval.

## Adversarial review

Four specialist reviewer jobs were attempted (authority, SMP/failure, distribution, dependencies); all failed with provider `usage_limit_reached` and returned no findings. **No independent-review PASS is claimed.** The integration owner completed a direct adversarial review using the source-grounded thread/distribution scout reports and the actual draft documents.

| Finding | Severity | Decision and resolution |
|---|---|---|
| Restore-before-publish alone permits acknowledged old-writer commits after the restored snapshot | Major | Accept: Spec 25 §6 and P06 require final fence/committed watermark, catch-up before publication and no reuse of fenced epoch. Added AC-27. |
| A relative timeout received after network delay cannot prove origin absolute expiry | Major | Accept: Spec 25 §7 and P08 distinguish origin end-to-end and destination receipt-relative deadlines; strict cutoff requires authenticated time/error bounds and commit check. Added AC-28. |
| A durable lifecycle result and kernel publication are not one atomic disk/scheduler transaction | Major | Accept: durable intent, stable creation key, kernel keyed publication and result reconciliation; after node reboot original create reports old incarnation gone rather than silently recreating. P06/P08, AC-29. |
| Multi-waiter take did not state eligibility and owner-exit race outcome precisely | Major | Accept: owner or explicitly handed same-Cell consumption handle; no lifetime transfer, exactly one cleanup/take release. Spec 25 §4 and P04; existing AC-10 covers it. |
| Single-writer fence alone did not state recovery authority's own failover rule | Major | Accept: fixed authority home, durable epoch/ledger before renewed admission, no timeout-based authority promotion. Spec 25 §7, P08 and AC-21/22. |

The initial review added no out-of-scope feature: those findings closed cases within the then-approved recovery/distribution scope. The subsequent maintainer-approved amendment adds bounded placement/autoscaling and application-wide accounting; it does not add consensus, transparent relocation or authority failover. Exact ABI/protected/physical gates remain unchanged. Independent automated assurance was unavailable for the initial review; accountable owner review remains required at implementation entry and actual ABI checkpoints.

## Document verification — initial baseline (AC-01..AC-29)

Executed an in-memory Python/YAML document-graph verifier from the repository root (no generated script or runtime source changes):

| Check | Observed result |
|---|---|
| Existing plan inventory | 161 entrypoints, 161 unique classified rows, no missing/extra row |
| Scoped locks | 38 unique locks; all have visible notice, `executionLock`, blocked execution status and nonempty scope |
| Reciprocal controller edges | 38 `blocks` ↔ `blockedBy` matches; controller does not wait on a locked predecessor |
| Implementation phase graph | 10 sequentially numbered pending phases; references resolve; acyclic |
| Acceptance ledger | 29 unique ordered EARS criteria, each maps to existing owner phase(s) |
| New document links | 17 new package/register documents; 201 Markdown links examined, local targets resolve; scoped lock notices also resolve |
| Negative controls | In-memory phase cycle, missing document link and removed reciprocal lock edge each detected |
| Verifier errors | 0 |

The verifier checks the new graph/metadata, not every historical link or the truth of old implementation claims. Original status/progress and earlier blockers were retained by surgical lock additions and the inventory owner; the verifier does not treat those status strings as runtime evidence.

No build, runtime test, lint or formatter was run for this documentation-only change. Prior local QEMU behavior is explicitly labeled reused evidence in `scout-report.md`; none qualifies the new contract. New files listed as future phase deliverables are planned work, not missing current implementation.

## Handoff

The planning/lock deliverable is complete. The next step is P01 exact ABI/migration/adoption inventory; implementation remains pending. Use the controlling plan, not an old locked phase. No automatic old-plan unlock, ABI confirmation, remote activation or production admission follows from this validation.

## Approved cluster follow-on review

Connection/session state stays in `net-broker`; IP/TCP remains service-net and local isolation remains kernel-owned. Placement is policy in a separate controller Cell, not a broker route fallback or a distributed kernel.

Owner adversarial checks for the amendment:
- Reserve durable application budget before remote creation, including pending/uncertain children; partition or reply loss cannot release a potentially live child's charge.
- Destination admission remains authoritative: aggregate application allowance cannot override node capacity, tier/artifact policy or scoped lifecycle permission.
- Controller restart reconciles durable intent and child creation keys before new allocation; fixed-home fenced controller has no timeout-based promotion.
- Scale-down stops new routing/admission and drains/classifies outstanding work before retirement; draining/uncertain children remain accounted.
- Stateless replicas or explicitly sharded state may scale; additional replicas cannot create another writer for the same mutable state domain.

Integrated in-memory Python/YAML document smoke after both edited slices:

| Check | Observed result |
|---|---|
| Existing inventory and scoped locks | 161 unique classified entrypoints; 38 reciprocal lock edges; blocked metadata retained |
| Phase graph | 10 pending phases; valid dependency references; acyclic; maximum phase length 73 lines |
| Current acceptance ledger | AC-01..AC-35 unique and ordered; all EARS criteria map to existing phases |
| Package links | 17 documents, 201 Markdown links examined; 0 broken local targets |
| Updated entrypoints | Contract/plan/spec pointer agree on AC-01..AC-35; inserted portfolio/focus links resolve |
| Negative controls | In-memory cycle and removed lock edge detected; missing target recognized; removed AC makes completeness check fail |
| Superseded scope | Automatic-placement exclusion removed; placement stays within one admitted cluster, not federation |

No build/runtime test/lint/formatter was run. This is document consistency and owner design review, not a placement, quota or two-node runtime PASS. All new runtime scenarios remain pending.

## Complete workload architecture review — Spec 26

The maintainer requested architecture first and implementation later. Spec 26 selects control/data-plane ownership, logical versus pinned endpoints, an exact bounded weighted-slot routing sequence, deterministic node placement and a demand-based scaling loop. AC-36..AC-43 append to the unchanged earlier criteria.

Owner adversarial review closed these gaps:
- Router-as-confused-deputy: preserve attested original subject and constrained delegation; destination grants still authorize the selected method/resource.
- Several routers/restarted ingress multiplying capacity: preallocated per-ingress shares sum within a replica envelope; fence old generation and reconcile retained work before share reuse.
- Route withdrawal racing a stale router: destination admission cutoff, not route-table removal alone, establishes safe drain.
- Unknown telemetry versus zero demand: required fresh source samples, no worker double counting, no downscale from missing data; bounded cold-start wake needs always-on charged control capacity.
- Application shrink below live use: pending target versus effective admitted ceiling; no forced work loss or temporary uncharged growth.
- Ambiguous submit versus balancing: one selected attempt, immutable concrete binding after submission, no hidden retry/hedging.
- Connection count versus readiness and capacity: live service export/state fence is required; sessions are bounded shared transports, not replicas.
- Live router gate depended on a later budget/controller gate: corrected internal execution order to P08-A → P08-C → P08-B → P08-D → P08-E; controller bootstraps explicit grants/membership before real router admission, with no fixture authority.

Integrated in-memory Python/YAML smoke passed: 161 unique classified legacy plans; 38 reciprocal scoped locks with blocked metadata; 10 pending phases with valid acyclic dependencies; 43 ordered EARS criteria with existing owner phases; 18 package documents and 215 Markdown links examined with zero broken local targets. The largest phase is 99 lines. Updated entrypoint links and AC range agree. Four negative controls reject a cycle, removed reciprocal lock, missing target and missing AC.

Final handoff check covered all six references to P08 internal order and the acyclic A → C → B → D → E dependency chain. Architectural bootstrap sequencing no longer requires routing on fake budget grants. These are document checks and integration-owner adversarial review only: no runtime, build, performance, independent security or production PASS is claimed. All implementation milestones remain pending.
