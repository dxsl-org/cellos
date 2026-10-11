---
phase: 8
title: "Protected distribution, lifecycle and placement/autoscaling"
status: pending
priority: P1
tier: thinking
dependencies: [4, 5, 6]
---

# P08 — Distribution without ambient cluster root

## Deliverable and entry gates

AC-20..AC-23, AC-28/29, shared node-admission AC-30, AC-31..AC-34 and AC-36..AC-42; P10 owns AC-35/43 workload qualification. [Spec 26](../../docs/specs/26-workload-placement-scaling-contract.md) is the detailed normative extension under Spec 25, not an independent AC namespace. All milestones below remain **pending**. One Cell stays on one physical/VM kernel instance; replicas are separate Cells, not remotely migrated threads or one Cell per request.

Design/adoption can proceed; protected Build and remote activation still await ADR-0008/0009, KMS/Silo AC-001..011 and AC-012 respectively. Exact API/manifest/persistent bytes require P01 Law-1 checkpoint 1; freeze/release requires checkpoint 2. No fixture keys, route, documentation approval or milestone waives these gates or any of the 38 locks.

## Related files / symbol-migration ownership

| Owner | Existing surface / proposed destination | Migration responsibility |
|---|---|---|
| P08-A transport integration | `cells/services/net-broker/src/{local_runtime.rs,local_runtime/,c2c_envelope.rs,c2c_ingress.rs,c2c_dedup.rs,transport.rs,connection_manager.rs,kms_dh.rs}`; `libs/ostd/src/cluster_endpoint.rs` | Inventory every concrete `RemoteEndpoint`/export, ingress-proof and correlation caller/test with P01; preserve concrete generation-pinned semantics, remove replaced dispatch paths in the owning slice. |
| P08-B router integration | **Proposed, not existing:** `cells/services/workload-router/src/{endpoint.rs,admission.rs,routing.rs,queue.rs,telemetry.rs}`; approved typed SDK/schema extensions | Own logical workload endpoint, original-subject propagation, schema/route binding, bounded dispatch/queue and counters; migrate opt-in callers only, never reinterpret a pinned endpoint. |
| P08-C/D controller integration | **Proposed, not existing:** `cells/services/placement-controller/src/{policy.rs,budget_ledger.rs,placement.rs,routing.rs,scaling.rs,reconcile.rs}` | Own application/workload policy, versioned route publication, durable reservations, desired replicas and reconcile; P01 finalizes exact symbols/files and all publisher/parser/test/doc migrations before creation. |
| P06/P08 lifecycle handoff | `cells/services/supervisor/`, P06 state authority and approved lifecycle schemas | Supervisor owns lifecycle; issuer/state authority owns effect validation and writer fences. Controller invokes typed endpoints, never a second lifecycle/state registry. |
| P08-A protected handoff | Existing KMS/Silo/service-net paths inventoried in P01 | Protected authority owns trust/relay TLS; broker owns sessions/reconnect/direct-relay transport pool; service-net owns IP/TCP. Router/controller gain no raw signer/socket authority. |

Adopt relevant locked C2C P04–P07 and KMS/Silo P04 slices without executing their old plans. Create isolated multi-node runner(s) under `scripts/` and behavioral oracles under `tests/integration/` during implementation; no existing Intel multi-node runner is assumed.

## Independently gated internal milestones — true execution order

**P08-A → P08-C → P08-B → P08-D → P08-E**, all pending, within P08's unchanged P04/P05/P06 dependencies. Labels identify slices, not lexical execution order: C's durable grants/shares and membership publisher are prerequisites for B's live routing. Each gate records exact revision, artifacts, positive/negative observations, approval and evidence ceiling; no milestone is passed by this plan. B's interface/oracle design may proceed earlier, but live admission cannot use fixture budget or a substitute publisher.

### P08-A — Authenticated transport and lifecycle substrate

1. Obtain real protected persistence/time/pending-key/replay/identity evidence. Bind source node/principal, audience, method/export/schema, destination generation, delegation/revoke epoch and quotas to live proof before dispatch. Fixture frame fields, uptime and local ephemeral K1 are not authority.
2. Keep complete relay TLS validation/framing in Protected Relay Authority and opaque end-to-end Noise/correlation in broker. ADR-0009 exact rejection is NotSubmitted; accepted/ambiguous handoff is Submitted. Relay-only authenticated two-node proof precedes separately admitted direct LAN; no absent-relay plaintext/direct bypass.
3. Bound origin operation, broker queues, authority handoff, destination admission and completion bytes/work with all-path ownership cleanup. Correlate cancel/reply to both node/incarnation bindings and exact operation/export/session. Rebooted brokers/services cannot answer retired work.
4. Subtract queue residence from remaining budget; origin end-to-end and destination receipt-relative timeouts differ without qualified cross-node time. Origin-cutoff-sensitive effects require authenticated expiry/error bounds and commit-time checks or refuse. Destination live export revocation and state-authority mutable commit fence remain authoritative.
5. Integrate P06 transactional request/outcome and durable-intent → keyed publication/reconciliation → durable-result lifecycle protocols. Lost replies reconcile the same key; expired outcome history remains uncertain, never re-execution. Scope artifact/template/tier/child ceilings and bounded-use grants; destination selects tier. Node reboot proves old child Gone, not implicit restart permission.
6. Fixed-home state authority recovers durable epoch before admitting work; no timeout promotion or cached pre-partition mutable grant. Close affected sessions on authority/broker identity uncertainty. Preserve physical-effect safety interlocks.

**Gate A:** isolated relay topology proves direct path blocked and relay succeeds; separately qualify direct LAN with identical semantics. Reject forged principal/peer/audience/method, stale generation/epoch, duplicate NodeId, invalid certificate/time/binding, replay, expired grant and quotas. Crash/revoke/cancel/partition around dispatch, commit and every lifecycle publication boundary proves correlated uncertainty, one intended child, no duplicate increment or stale-writer effect.

### P08-B — Workload endpoint, routing and bounded admission

1. Add an explicit logical endpoint for replication-compatible typed methods only. Stateless eligibility is opt-in; shard/writer/session affinity resolves an approved ownership map, never automatic reshard/writer cloning. Concrete endpoints retain pinned generation semantics.
2. Admit one workload-router Cell at each selected ingress node. Kernel-attested original subject and delegated exact method/export scope survive routing; destination independently authorizes subject and effect. Controller owns versioned membership, not a per-request hop; router owns request selection, not ambient proxy authority.
3. Cache a bounded route/candidate table bound to service schema, route version, controller epoch/sequence, exact member generation and local monotonic receive-age validity. Publication validation is off the per-request path; no global scan or repeated signature verification for local dispatch. Unavailable/stale routes refuse.
4. Precompute a bounded weighted slot cycle: stable replica-identity order, positive integer concurrency-derived weights divided by GCD, each replica repeated its normalized weight; policy bounds both member count and total slots. Cursor advances on every inspected slot, skips ineligible/capped members and inspects at most one full cycle per decision. Atomically reserve local per-router/per-replica in-flight share before submit; destination cap is authoritative. Exclude draining/suspect/stale/wrong-affinity members. Local preference is only an explicit bounded weight; load is a hint, not authority.
5. Bound queue request count, bytes and residence by caller deadline and policy. Freeze the selected concrete target at submission: exactly one selected attempt; destination refusal is returned, never hidden retry, hedging or replay of ambiguous work. Queue expiry cannot execute later; no queued replica target is treated as Ready.

**Gate B:** deterministic oracle covers unequal weights, capped replicas, every candidate unavailable, wrong schema/subject, pinned generation, affinity and route expiry. Duplicate/lower route or health sequences cannot renew freshness. Observe per-router/destination caps; router restart cannot reset unresolved in-flight usage to zero or borrow another ingress's share. Queue overflow and late readiness return explicit overload/unavailable/timeout without later effects.

### P08-C — Nested budgets and deterministic placement

1. Admit finite policy units: application/workload IDs; allowed node/artifact/template/tier/architecture and state-locality constraints; child method/export/authority ceilings; total/per-workload/per-node envelopes; per-replica charge; min/max replicas; router/controller always-on envelopes; routing weights/concurrency shares/member/normalized-slot limits and optional bounded locality bias.
2. Declare budget vector units: replica/Cell slots, memory bytes, CPU execution-time entitlement per common replenishment period plus burst, threads, capability slots/metadata bytes, operation/completion slots and queued bytes. No MHz, utilization percentage or throughput substitutes for CPU entitlement. Router/control/telemetry envelopes count toward the same application budget.
3. Fixed-home fenced controller durably preallocates workload/node/router shares; sum of assigned shares plus unassigned capacity fits application totals, and ingress shares sum within each replica envelope. Static shares are the baseline: no automatic borrowing; reassign only after old-generation admission is fenced and retained work reconciled, never by zeroing counters after router restart. Each local admission checks its share without per-request durable transactions; destination/node limits remain independent. Reserved + live + draining + unknown charges fit effective application budget; no double counting between state categories.
4. Atomically reserve complete charge and unique create key before spawn. Destination validates application/subject/reservation/key/controller epoch and atomic keyed admission; no double claim/reuse. Filter allowed artifact/tier/architecture/resources/state locality and fresh capacity; rank fewer application-charged replicas on node, then greater normalized resource headroom, then stable NodeId. Fresh hints are not a cluster-free-resource guarantee; atomic destination refusal/reconcile handles TOCTOU, no implicit overload spillover.
5. Shrink below current charged use becomes pending target, never retroactive revocation/drop or a claim the smaller ceiling already holds. Suspend new creates until safe drain/reconcile installs it. Resize reserves positive vector delta before publication; release decreases only after authoritative enforcement/terminal evidence. Durable key closure must prevent delayed create after reclaim.
6. Bootstrap the fixed-home controller and approved ingress envelopes with supervisor-held grants, outside the replicas they manage; provide authoritative versioned membership for Ready replicas created through this ledger before Gate B live routing. No circular dependency on autoscaling or on a routed workload endpoint to start its own authority.

**Gate C:** independent application and node overflow, forged reservation, double claim, wrong destination/template/tier/subject, stale epoch, deterministic placement tie-break, destination refusal and pending shrink all preserve budget and authority. Reservation/flush failure submits no create; no policy field/default/byte encoding is considered approved until P01 inventory and exact checkpoints.

### P08-D — Demand-driven scaling and reconciliation

1. Finalize finite policy: targetConcurrencyPerReplica (requests), min/max, sample-window duration and receive-age limit (monotonic time), consecutive up/down windows, cooldown duration, up/down replicas per rate window, pending-create count, queue count/bytes/deadline, wake coalescing count/window and route health/publication validity. P01 supplies approved numeric profile values and overflow/reject behavior; no unbounded defaults.
2. Collect nonoverlapping authorized ingress queued + in-flight request counts per workload; one request has one ingress owner and is not counted again at worker/destination. Every source has identity, epoch, monotonic sequence, window and local receive age. Stale/missing/reset/duplicate windows cannot become zero demand; invalid coverage freezes scale decisions.
3. For complete fresh samples compute `ceil(demand / targetConcurrencyPerReplica)`, clamped min/max; apply consecutive-window hysteresis, cooldown, rate, pending and budget limits. This is a measured concurrency target, not promised capacity. Include preparing/unknown reservations in limits; reconcile unresolved creates before new mutation.
4. Recover durable controller epoch/ledger before changes. Reserve → submit keyed create → reconcile durable result → restore/state-fence/Ready → publish exact generation. Unknown reply/cancel/partition/restart retains charge and same key; release only authenticated Gone/absence with authoritative key retirement against delayed publication.
5. Min-zero requires admitted always-on router/controller capacity and bounded coalesced wake; queued requests can expire before cold start and must never run afterward. No budget/placement changes on controller-home outage. Existing routing survives only while publication validity AND destination live authority/state fence remain valid; stale grant never authorizes a new mutable call.
6. Downscale removes new routing and marks Draining, then quiesces admitted operations/grants/outcomes and safe state ownership before retire/Gone/release. A stale route cannot bypass destination draining admission; already admitted work is not killed for a desired target. Partitioned/unsafe-writer drains remain charged.

**Gate D:** deterministic complete samples and fault traces below prove load causes automatic create and Ready publication, not manual replica provisioning; missing telemetry, no capacity and late cold start explicitly refuse/freeze. All outcomes preserve original request identity/deadline/authority and one-attempt semantics.

### P08-E — Failure qualification and algorithm oracle traces

The following are required future executable traces, **not observed results**. Record each transition's input, cursor/sample/ledger state, selected target or refusal, expected/observed effect, and charged vector.

| Trace | Inputs and expected transition/oracle |
|---|---|
| Unequal weights, saturation, affinity | Fixed Ready A/B (stable A before B), weights 2:1, cursor 0, spare capacity: slots A,A,B select A,A,B,A,A,B. At cursor 0 cap A: inspect/skip slots 0/1, select B at 2 and advance to 0. Cap B too: at most three inspections, then bounded queue/overload/expiry. Affinity→A with A capped never spills to B. A selected submission's refusal returns directly, never selects another slot for that request. |
| Placement and readiness | Filter disallowed/state-ineligible/stale-capacity nodes. Among eligible nodes with charged counts 2/1 choose 1; equal counts choose greater normalized headroom, then stable NodeId. Reserve and submit; no request reaches Preparing child. Destination local-capacity loss refuses/reconciles rather than forcing placement. |
| Demand and stale samples | Example profile target=4, min=1, max=4, two consecutive windows, up-rate=1: fresh demand 9 gives raw target 3 twice, then one allowed create from desired 1; cooldown/pending prevent an extra create. Replay/old epoch/missing ingress or expired sample freezes both directions, not zero-driven downscale; later complete low windows permit only configured safe downscale. |
| Cold start and expiry | min=0 with admitted router/control envelopes; concurrent accepted wake demand coalesces under cap into one keyed intent. Request deadline expires while child Preparing: terminal timeout, zero later execution after Ready. Overflow wake/queue refuses explicitly; no always-on envelope means min-zero policy rejected. |
| Delayed spawn and dropped reply | Reserve key K → delayed submit/reply loss → unknown charge. Restart/partition cannot mint a replacement K2 or reclaim K. Empty lookup alone retains charge; authoritative key closure/Gone permits release, and subsequent delayed K publication is rejected. |
| Drain/route race and outage | Publish removal while old route remains at ingress: destination rejects new admission after its drain boundary, no reroute/replay; previously admitted request completes or preserves uncertainty. Controller outage preserves only still-valid routes and live destination fences; expiry/refence refuses, no mutation/promotion. Gone proof finally releases charge. |
| Dynamic budget shrink | Effective memory ceiling 400 MiB includes 100 MiB total router/control envelope and three 100 MiB workers; target 200 MiB is pending, no create/drop/revocation. Safely drain two workers, releasing each charge only on terminal proof; then install effective 200 MiB ceiling. Unknown/draining charge blocks installation; resize growth reserves positive vector delta first. All other budget dimensions remain independently enforced. |

**Gate E:** repeat A–D and these traces under controller/destination/router restart, partitions, stale/replayed routes/health/telemetry, revoke and lost replies on relay and separately qualified direct LAN. Reassignment before old-router fencing/reconciliation must fail; restart cannot erase retained in-flight charges and replay cannot refresh expiry. Retain full serial/host packet/ACL/artifact evidence; host simulation alone cannot qualify remote/runtime claims. Hand off P10 AC-43 multi-node load-driven spawn/balancing evidence; 200 clients alone is insufficient.

## Risk / rollback

Availability yields to current authority/fencing and durable budgets. No federation, consensus, timeout-promoted controller, multiwriter, DSM or transparent migration. Remote effects, protected epochs and ledger transactions cannot be undone by binary revert: close admissions, retain outcomes/unknown charges, reconcile children/keys and advance authority under policy. Missing protected prerequisites leave the profile disabled and gates unpassed, never fake success.

## Assumptions and deviation log

Public RemoteEndpoint currently refuses transmission; ingress fixtures are not end-to-end proof. Proposed router/controller paths above are future targets, not source findings. Protected Intel authority and physical multi-node qualification remain external gates. This documentation amendment adds detailed workload control/data-plane design and AC-36..AC-43 without changing ten phases, dependencies, locks or pending status.
