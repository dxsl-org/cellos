---
phase: 10
title: "End-to-end profile qualification and fair Linux comparison"
status: pending
priority: P1
tier: thinking
dependencies: [2, 3, 4, 5, 6, 7, 8, 9]
---

# P10 — Qualify the contract, not just the build

## Deliverable and requirements

AC-25/35/43 plus qualification of every AC-01..AC-50 under Spec 25 v1.3 and Spec 26 v1.1. The checkpoint explicitly amends affected earlier semantics. Evidence must cover the independent Budget Authority and complete router/placement/scaling contract, with bounded costs and fair Linux comparison. Runtime and production remain pending, independently gated.

## Related files / ownership

- Existing Intel PC lane `.agents/261004-1957-x86-pc-lane/plan.md` and HCL qualification files; adopt needed unfinished slices, not the locked scheduling authority.
- `docs/evidence/`, `tests/integration/`, runtime fixtures/runners and approved profile/build configs from P02..P09.
- `docs/specs/{15-kernel-boundary,17-ipc-wire-contract,20-unified-ipc-contract,22-native-domain-cell-implementation-gate,25-cell-kernel-authority-contract,26-workload-placement-scaling-contract}.md`.
- `docs/security-model.md`, runtime/SDK/authority guides, `.agents/plan-portfolio.md`, `docs/roadmap/current-focus.md`, lock register and phase evidence records.

## Implementation steps

1. Rebuild exact default/native-only/compatibility/domain/guest/remote test profiles. Run a combined consumer workload: concurrent native/C++ threads, authorized resource access, provider crash/recovery, revoked grants and distributed uncertain outcomes. Reuse phase oracles rather than duplicate wiring tests.
2. Exercise **actual hundreds of concurrent clients (at least 200, recording observed overlap)** issuing independently correlated requests against a bounded worker-pool service on one node, then the same workload across selected replicas within the admitted cluster. This is one application's cross-node placement, not federation. Clients/requests are not Cells: record actual Cell/thread/replica/session counts separately. A serial loop or a configured client count without simultaneous live requests is not this witness. Fix message sizes, immutable request IDs, authority, deadlines, effect/durability semantics and cancellation/uncertainty rules across both runs; report refusal/overload rather than inventing success or promising throughput or transport-level exactly-once delivery.
   **AC-43 is a separate actual balancing/autospawn witness, not satisfied by 200 clients:** begin with policy min replicas on one admitted node and finite router/control envelopes; hold explicit sustained authorized demand across ingress nodes above the approved target windows. Observe telemetry → desired target → durable reservation/key → automatic remote spawn on another admitted node → restore/fence/Ready → route publication, without an operator provisioning the additional worker. Under stable membership/readiness and unequal approved weights, record per-replica selections/completions, actual concurrent occupancy and bounded queue pressure; compare selection counts with deterministic weighted-cycle oracle, not a claim of equal throughput. Show both nodes serving new work with preserved authority/schema/deadline semantics.
   Then hold low demand through configured down-windows/cooldown and observe new-route removal → safe drain → Gone/key retirement → charge release. Repeat with one capped replica, affinity-pinned work, destination admission refusal, missing telemetry, controller-home/worker partition, delayed/lost create reply, dynamic budget shrink and late cold-start readiness. No capped/affinity failure triggers hidden replay; unavailable demand yields bounded refusal/expiry, not fabricated immediate relief. Min-zero subcase proves coalesced wake and no post-deadline execution.
3. Collect node and nested application/workload/router Cell/replica/thread/memory/aggregate-CPU/capability/operation/completion/queue/session accounting before load, saturation and full quiescence, including router/controller/telemetry envelopes, preallocated shares and pending/unknown/draining charges. Report bytes, slots/counts and CPU execution-time per common period, not MHz/throughput. Inject each node/application limit, positive-delta resize and below-use pending shrink; prove no premature release, forced eviction, delayed key publication after reclaim or global per-request durable transaction. Retained resources have named ownership; intentional quarantine is not hidden leak-free success.
4. Qualify one exact physical Intel configuration: boot/console, wired network, persistence across cold boot and applicable DMA isolation. Follow existing PC procurement/authorization gate. Qualify node B independently before physical two-node claims; QEMU is software evidence only.
5. Run same contract scenarios on actual admitted physical/VMX profiles, with fault/reboot/network-partition/revoke and artifact measurement. Include controller-home outage/no timeout promotion, stale controller fence, destination admission refusal, duplicate/lost spawn reconciliation, min/max/rate/threshold/cooldown bounds, Ready-only routing and drain with admitted work/single-writer state. Repeat relay-first and then separately qualified direct-LAN routes without replaying uncertain requests. Missing protected authority, machine or independent approval leaves a named unpassed row; no lower-ceiling substitution.
6. Compare same-hardware Linux implementations at matched operation, message sizes, concurrency, state durability and failure semantics. Record kernel/config/compiler, CPU affinity/frequency, memory, warmup/repetitions and raw samples. Include DAC/root baseline and an explicit least-privilege configuration (namespaces/capabilities/seccomp/LSM or Landlock as applicable), not a deliberately insecure strawman.
7. Report p50/p95/p99/max, throughput, CPU time, peak/resident memory, authority/queue metadata bytes, revoke cutoff/quiescence latency, recovery time and fault correctness. Include achieved client overlap; demand sample ownership/epoch/sequence/age; per-node/per-replica routing and occupancy; target versus actual/Ready replicas; reservation/create/Ready/drain/Gone timestamps; refused/uncertain/committed counts and complete charged vectors. Security claims list tested properties; 200 clients or a CPU quota implies neither 200 Cells nor throughput.
8. Optimize only a measured bottleneck without changing admission/failure semantics. Shared ring/zero-copy must retain ownership/revoke proof; if it fails, keep copied transport and report measured costs rather than weakening checks.
9. Complete migration census: no executable old path/name authority, anonymous state restore, ambient POSIX linkage or raw-address capability bypass remains for cutover consumers. Public retired IDs remain reserved; docs and examples use the new contract. Update Spec 20 only through its own ratification, not by removing its warning blindly.
10. Present phase-specific checkpoint-2 and release/production evidence to accountable maintainer. Close/rebase locks one slice at a time with explicit decision; historic completed evidence remains historical. Do not unlock the entire portfolio by finishing documentation.

## Verification / success criteria

- [ ] Every AC row has actual positive/negative runtime evidence at the required ceiling, or the whole program remains explicitly incomplete.
- [ ] Cross-phase two-CPU, replacement, VFS/NET, POSIX-free/native C++, two-node and Intel guest scenarios run from matching built artifacts without test-only security bypass.
- [ ] Repeated load/crash/revoke leaves bounded accounted resources; no old incarnation commits or completes work for a successor.
- [ ] At least 200 simultaneously live clients are witnessed in both local and cluster workloads with unchanged request semantics.
- [ ] Node and application quota boundaries, unknown-create accounting and controller fencing survive crash/restart/partition without oversubscription, duplicate child publication or timeout-based budget release/promotion.
- [ ] Scaling obeys min/max/rate/threshold/cooldown; only Ready replicas receive new requests, and draining replicas finish admitted work or preserve explicit uncertainty/state fencing before retirement.
- [ ] AC-43 actual sustained load automatically spawns a newly Ready worker on another admitted node and routes new work across both with measured weighted distribution/caps.
- [ ] P08-A→C→B→D→E oracle traces run at the claimed ceiling, with C's durable budgets/real shares/bootstrap membership preceding B's live routing, never mock grants.
- [ ] Qualify AC-44..AC-50 at the claimed runtime ceiling: late unseen high-sequence challenges; old-generation unresolved demand/new zero; affine/heterogeneous wrong-domain refusal; malicious controller distinct-key attempts, genesis crash and live-claim partitions; immutable-max resize; forged Ready provenance; altered payload/deadline and second-target origin attenuation.
- [ ] Exact physical Intel and production rows have their own evidence/approval; all benchmark data includes semantics/configuration and raw samples.
- [ ] Documentation graph, lock graph, public contract ledger and full affected caller census are consistent.

Correction 2026-10-16: prior checkmarks and cited standalone-model/storage-boot receipts did not establish these criteria. Acceptance remains pending; historical observations are not runtime qualification.

## Risks and rollback

Do not let performance pressure convert revocation into eventual best-effort, remote uncertainty into retries, or quota pressure into an unfenced controller/unsafe drain. Prior known-safe image + cold boot is the binary rollback; data/epoch/budget-ledger changes require approved recovery/forward migration. Once a remote/physical side effect occurs it cannot be undone by a feature flag. Re-close an affected profile on regression while keeping unrelated qualified local profiles available.

## Assumptions and deviation log

No blanket speed/security superiority to Linux or CPU/throughput capacity is assumed. Physical Intel, genuine VMX and production approval remain external gates. Qualification covers all 50 criteria, including actual balanced load-driven spawn; ten phases/dependencies and 38 locks stay unchanged. Binary rollback cannot undo external effects or durable fence advancement; restore/forward migration is required.
