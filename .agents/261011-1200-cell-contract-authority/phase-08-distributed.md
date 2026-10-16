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

AC-20..AC-23, AC-28/29, shared AC-30, AC-31..AC-34, AC-36..AC-42, AC-44..AC-47 and AC-49/50; shared AC-48 consumes P03 immutable maxima. P10 owns AC-35/43 and qualifies all AC-01..AC-50. [Spec 26](../../docs/specs/26-workload-placement-scaling-contract.md) details Spec 25, not another AC namespace. All milestones remain **pending**. One Cell stays on one physical/VM kernel instance; replicas are separate Cells, not remotely migrated threads or one Cell per request.

Design/adoption can proceed; protected Build and activation await ADR-0008/0009, KMS/Silo AC-001..011 and AC-012 respectively. P01 Law-1 checkpoint 1 approves exact public/manifest/persistent bytes before editing; checkpoint 2 governs freeze/release. Architecture/fixture approval waives none of these gates or 38 locks. This v1.3 checkpoint amendment explicitly revises affected existing AC semantics, preserves IDs and appends AC-44..AC-50.

## Related files / symbol-migration ownership

| Owner | Existing surface / proposed destination | Migration responsibility |
|---|---|---|
| P08-A transport | `cells/services/net-broker/src/{local_runtime.rs,local_runtime/,c2c_envelope.rs,c2c_ingress.rs,c2c_dedup.rs,transport.rs,connection_manager.rs,kms_dh.rs}`; `libs/ostd/src/cluster_endpoint.rs` | Inventory every concrete endpoint/export, proof/correlation caller/test with P01; preserve generation-pinned semantics and remove replaced dispatch paths. |
| P08-C Budget Authority | **Proposed, not existing:** `cells/services/budget-authority/src/{bootstrap.rs,ledger.rs,reservations.rs,claim.rs,fence.rs,recovery.rs}` | Own independent trusted enforcement, issuer-precommitted genesis adoption, reservation/claim/closure and durable logical application ledger. P01 inventories app issuer and supervisor publisher/parser/storage callers. |
| P08-C/D controller | **Proposed, not existing:** `cells/services/placement-controller/src/{policy.rs,budget_client.rs,placement.rs,routing.rs,scaling.rs,reconcile.rs}` | Own admitted policy, desired replicas and logical route publication; consume Authority grants, never own/mint enforcement ledger. |
| P08-B router | **Proposed, not existing:** `cells/services/workload-router/src/{endpoint.rs,admission.rs,routing.rs,queue.rs,telemetry.rs}`; approved typed SDK/schema extensions | Own opt-in endpoints, immutable origin-proof propagation/selection attenuation, Ready validation, bounded queues/counters/challenges. |
| P06/P08 lifecycle | `cells/services/supervisor/`, P06 state authority and approved lifecycle schemas | Supervisor consumes keyed grants; destination supplies authenticated Ready evidence and independent local admission; issuer/state authority owns effects/writer fences. No duplicate lifecycle/state registry. |
| P08-A protected | Existing KMS/Silo/service-net paths inventoried in P01 | Protected authority owns trust/relay TLS; broker owns bounded sessions/reconnect/direct-relay pool; service-net owns IP/TCP. No router/controller raw signer/socket authority. |

P01 fixes exact symbols/files, all publisher/parser/caller/test/doc migrations and approval rows before creation. Adopt relevant locked C2C P04–P07 and KMS/Silo P04 work without running old plans. Future isolated multi-node runner(s) under `scripts/` and behavioral oracles under `tests/integration/` are implementation deliverables, not existing Intel qualification.

## Independently gated milestones — true execution order

**P08-A → P08-C → P08-B → P08-D → P08-E**, all pending inside unchanged P04/P05/P06 dependencies. C first bootstraps/recovers the independent Budget Authority, then controller and charged ingress envelopes, then real shares and destination-backed membership before B live routing. B interface/oracle design may precede C; live admission cannot use fixture grants or substitute publisher. Each gate records revision/artifacts, approvals, positive/negative observations and evidence ceiling.

### P08-A — Authenticated transport and lifecycle substrate

1. Obtain real protected persistence/time/pending-key/replay/identity evidence. Bind live proof to source node boot/Cell incarnation, origin operation/request identity, canonical typed payload or digest, workload/service/method/schema/authority scope and original deadline. Request grants authorize that exact origin operation, never fresh minting/replay. Fixture CellId, uptime and ephemeral K1 are not authority.
2. Complete relay TLS validation/framing stays in Protected Relay Authority; opaque end-to-end Noise/correlation stays in broker. ADR-0009 exact rejection is NotSubmitted; accepted/ambiguous handoff is Submitted. Relay-only two-node proof precedes separately qualified direct LAN; no plaintext/absent-relay bypass.
3. Bound origin operation, queues, handoff, destination admission and completion bytes/work with all-path cleanup. Cancel/reply binds both node/incarnations and exact operation/export/session. Kernel/local record checks avoid crypto per local call; remote authenticated channels carry protocol-bound proofs, never trust payload CellId. Destination correlates/deduplicates replay, never creates another effect.
4. Subtract queue residence from remaining budget; origin end-to-end and destination receipt-relative timeouts differ without qualified cross-node time. Origin-cutoff-sensitive effects require authenticated expiry/error bounds and commit-time checks or refuse. Router cannot extend original deadline; live export revocation and mutable state commit fence remain authoritative.
5. Integrate P06 durable intent → keyed publication/reconcile → durable result. Lost replies use the same key; expired history remains uncertain, never re-execution. Destination selects admitted tier; node reboot proves old child Gone, not implicit restart permission.
6. Fixed-home state authority recovers durable epoch before admission; no timeout promotion or cached mutable grant. Close uncertain authority/broker sessions; preserve physical interlocks.

**Gate A:** relay topology blocks direct and permits relay; separately qualify direct LAN identically. Reject forged peer/principal/audience/method, stale incarnation/epoch, duplicate NodeId, certificate/time failure, replay, quota and expiry. Crash/revoke/cancel/partition around dispatch/commit/publication proves one intended child, no duplicate/stale effect. Alter payload/deadline/operation under copied origin proof and unauthorized selection attenuation must fail.

### P08-C — Independent Budget Authority, genesis and deterministic placement

1. Use an independently isolated fixed-home Budget Authority principal, not controller-owned state. App budget issuer durably precommits logical application genesis, approved maximum vector and control/Authority/controller/router envelopes before any spawn; issue one-use keyed bootstrap admission reservation. Supervisor may consume that grant, not mint/reinitialize an empty ledger. Crash-query/recover the same key idempotently; no controller placement mutation before Authority recovers/claims genesis.
2. Declare finite allowed nodes/artifacts/templates/tiers/architecture/state locality, child authority/method/export ceilings, min/max and nested workload/node/router shares. Vector units: Cell/replica/thread/capability/operation/completion slots, memory/metadata/queue bytes and aggregate CPU execution-time entitlement per common replenishment period plus bounded burst; no MHz/throughput. Authority/controller/router/telemetry envelopes consume application totals.
3. Authority durably reserves full vector and unique create key, authenticating non-mintable application/destination/key/vector/epoch-bound grants. Destination must make a **live atomic claim with Authority before its own publication** and independently enforce local/tier/subject/artifact bounds. No delegated offline budget lease. Uncertain claim remains charged and closes new admission; known already-claimed child recovers keyed without second claim.
4. Authority restart recovers the same durable logical ledger with monotonic fence before fresh grants; no self-promotion, second allowance or reinitialized balance. Reserved + live + draining + unknown fit the effective application ceiling without category double count. Controller can maliciously misuse its admitted policy authority within that ceiling but cannot increase it by distinct reservations.
5. Authority preallocates nested static shares whose sums fit application/replica bounds. No automatic borrowing or per-request durable budget transaction. Reassign only after old generation is fenced and retained operations reconciled. Controller placement filters policy and challenge-fresh capacity; ranks fewer application-charged replicas, greater normalized headroom, then NodeId. Destination refusal/reconcile handles TOCTOU; hints promise no free capacity.
6. Distinguish immutable incarnation resource/authority maxima from mutable currently admitted allocation ≤ maxima. Growth reserves positive vector delta with Authority and destination before publication; above-max growth refuses the existing Cell and requires explicitly budgeted replacement. Below-use shrink stays pending until safe use/drain/reconcile permits enforcement and release; key closure fences delayed publication.
   Shared AC-48 consumes P03's qualified local max/current-limit primitives; this milestone supplies the real Authority reserve/live-claim growth and replacement accounting witness. P03 cannot close this management-path criterion. No fixture grant enables application-managed resizing.
7. Only after genesis adoption, bootstrap controller and ingress from charged grants outside managed replicas. Controller publishes membership using destination/assigned issuer Ready evidence bound to application, reservation/create key, measured artifact, tier, export/schema and exact generation; no routed workload dependency to start its own authority.

**Gate C:** malicious admitted controller issues distinct reservations to multiple nodes: aggregate exceeds max only by refusal, not minting. Inject issuer precommit/flush, bootstrap spawn/claim/reply crashes and Authority restart/failure; recover same genesis/key/ledger, retain uncertainty, refuse offline publication and second allowance. Reject forged/double/wrong-node grants, stale fences, above-max resize and forged Ready; node/application overflow, tie-break, positive-delta resize and pending shrink preserve independent bounds. No policy bytes/defaults approved outside P01 checkpoints.

### P08-B — Workload endpoint, routing and bounded admission

1. Logical endpoints opt into replication-compatible typed methods; concrete endpoints stay generation-pinned. Shard/writer/session affinity follows approved ownership map, never reshard/writer clone. Destination independently authorizes original subject/effect; controller is logical publisher, not per-request hop.
2. Router verifies assigned destination/issuer Ready evidence against every member's artifact/tier/key/reservation/application/export/schema/exact generation. Controller's assertion of Ready alone is insufficient. Compromised admitted destination is outside trusted-kernel scope; controller remains untrusted for budget enforcement/provenance.
3. Routes, health, telemetry and capacity use receiver-initiated bounded challenge rounds: nonce binds receiver incarnation, workload/policy and expected source set; receiver records local start. Source generates/authenticates sample/publication only after receiving challenge. Accept once before start+maximum round lifetime; TTL runs from start, never arrival. Duplicate/lower or delayed unseen higher sequence cannot renew validity; repeated bounded rounds, not packet drip, renew. No cross-clock assumption; destination live authorization/fences still required.
4. Cache bounded route/candidate table with schema/version/controller epoch/sequence/exact member generation. Publication checks stay off local per-request path. Selection attenuation binds unchanged origin proof to route version and a Ready member/epoch; cannot alter payload/deadline or mint a request. Bounded GCD-normalized positive weighted cycle in stable identity order advances on every inspection, at most one full cycle; atomically reserve local ingress/replica share, respect destination cap, skip draining/suspect/stale/wrong-affinity. Same-capacity locality bias is permitted.
5. Bound queue count/bytes/residence by original deadline and policy. Freeze concrete target at submission, exactly one selected attempt; refusal/ambiguous work returns without retry/hedging/replay. Expired queue never executes after late Ready.
6. Origin kernel/approved operation issuer seals one target in immutable operation-owned attempt state and returns only that destination attenuation on repeat retrieval. Router cannot sign selection or obtain a second member/request identity. Destination-local dedup is insufficient across two destinations; preserve origin-local refusal and no global per-request ledger hop.

**Gate B:** unequal weights/caps/affinity, wrong schema/subject/provenance and expiry obey deterministic bounded selection. Forged Ready or changed payload/deadline/operation/route attenuation fails. Expired challenge with authentic unseen higher sequence stays expired; restart cannot reset shares or borrow; overflow/late Ready yields explicit refusal/timeout.

### P08-D — Homogeneous autoscale domains and generation reconciliation

1. Finite profiles define homogeneous interchangeable resource/template/concurrency domains with target T, min/max, nonoverlapping windows, challenge lifetime, consecutive up/down windows, cooldown, replica-per-window rates, pending/wake/queue caps. Only requests eligible for that domain contribute scalable demand. Affine/stateful/noninterchangeable demand reports unsatisfied/no eligible capacity, never pooled to spawn wrong replica.
2. Heterogeneous weighted pools remain manually routable but do not use homogeneous count formula unless explicitly partitioned into independent approved domains; all domain sums share the application budget. Changing resource class/template/concurrency removes replica from autoscale domain pending approved classification; no implicit T/capacity reinterpretation.
3. Versioned required ingress-generation set counts queued+in-flight once, never worker double count. Prior unresolved generation remains required; missing/reset/incomplete coverage freezes **both directions**, never guessed counts. New generation's fresh zero cannot replace old until authoritative operation reconciliation and fenced retired-share proof permit atomic one-time replacement. Samples belong to challenge bounds/nonoverlapping windows.
4. Complete fresh eligible demand computes ceil(demand/T), clamped min/max with hysteresis/cooldown/rate/pending/budget gates. Recover controller policy epoch and Authority ledger/fence before mutations; reconcile unknown same-key creates first. Reserve → live destination claim → create/reconcile → restore/fence/Ready evidence → publish exact generation.
5. Min-zero requires already charged Authority/router/controller capacity and bounded coalesced wakes; expired requests cannot execute after cold start. Controller outage closes policy mutations; Authority outage closes new claims/resizes/share changes. Unchanged routing requires still-valid challenge publication AND destination live authority/state fence, never offline budget permission.
6. Downscale removes new routing, marks Draining, quiesces admitted operations/grants/outcomes and safe writer ownership, then Gone/key closure/release. Old route cannot bypass destination drain; unsafe/partitioned drains stay charged and admitted work is not killed.

**Gate D:** complete homogeneous windows cause actual automatic create/Ready, not manual provisioning. Restart with old unresolved requests and new zero freezes both directions until proof; affine/heterogeneous wrong-domain demand never spawns incompatible capacity. Missing telemetry, no capacity and expired cold start refuse/freeze explicitly.

### P08-E — Required future adversarial traces, not observed results

| Trace | Inputs and expected transition/oracle |
|---|---|
| Weighted saturation/affinity | A/B weights 2:1 → A,A,B cycle; capped A skips both slots to B, both capped gives at most three inspections then refusal/queue. A-pinned work never spills to B; selected refusal never retries. |
| Placement/readiness | Filter disallowed/stale/ineligible nodes; charged counts 2/1 prefer 1, ties headroom then NodeId. No Preparing dispatch; forged artifact/tier/key/application/generation Ready evidence rejects even from admitted controller. |
| Malicious controller/budget | Two distinct keys on separate nodes cannot exceed remaining vector; live Authority claims enforce aggregate independently. Authority partition forbids new offline publication; claimed known child recovers once, uncertainty stays charged. |
| Genesis crash/restart | Crash at precommit, bootstrap submit, claim/publication and reply: same durable genesis/key lookup, no uncharged control Cell or empty reinit. Authority restart advances fence with unchanged logical balance; failure admits no second allowance/controller mutation. |
| Challenge delay | Withhold authentic unseen higher route/health/telemetry/capacity sequence past receiver start+bound; reject without resetting TTL/down-window. Wrong nonce/incarnation/source set or reuse fails; timely next bounded round alone renews. |
| Restart unresolved demand | G1 retains eight requests, G2 reports zero: G1 remains required, both scale directions freeze. Only terminal reconciliation plus retired-share fence atomically replaces once; later complete low windows can downscale. |
| Homogeneous demand/domain | T=4, min=1,max=4, two windows, up-rate=1: eligible 9 twice permits one create toward 3. Nine A-affine requests or incompatible template capacity cause no wrong-domain spawn; resize/class change needs reclassification. |
| Cold start/late create | Charged min-zero wakes coalesce to one key; deadline expiry while Preparing gives no later execution. Dropped reply/delayed K stays charged; empty lookup alone cannot reclaim or mint K2; terminal key closure rejects delayed K. |
| Drain/outage/shrink/resize | Old route loses new admission at destination drain; prior work completes/uncertain. 400 MiB (100 control+three 100 workers) → pending 200 until two Gone releases; unknown blocks. Positive delta ≤immutable max claims before growth; >max refuses existing Cell, only explicit budgeted replacement. |
| Immutable origin proof | Changed payload/deadline/operation/member/epoch fails. Origin issuer refuses second destination attenuation for the same operation; repeated retrieval returns only first binding. Attempt delivery to two replicas cannot produce two effects. Local immutable records add no per-local-call crypto/global budget hop. |

**Gate E:** run A–D/traces under router/controller/Authority/destination restart, partition, revoke and lost replies on relay then separately qualified direct LAN. No share reassignment before old-generation fence/reconcile, packet-drip renewal, authority promotion or erased charge. Retain serial/packet/ACL/artifact evidence; host simulation alone is not remote proof. P10 AC-43 needs measured multi-node autospawn/balancing, not 200-client count.

### Verified Milestones and Evidence Status

- [ ] **Gate A:** Actual authenticated relay/proof/dispatch/replay witnesses remain missing; codec verification alone is insufficient.
- [ ] **Gate C:** Actual native Authority genesis/live claim/closure/backend and budget witnesses remain missing; isolated journal gates are insufficient.
- [ ] **Gate B:** Actual weighted router/Ready/attempt/admission witnesses remain missing; the historical test-only round-robin model is not evidence.
- [ ] **Gate D:** Actual telemetry-driven spawn/drain and old-generation reconciliation witnesses remain missing; formula models are insufficient.
- [ ] **Gate E:** Actual multi-node restart/partition/revoke/lost-reply traces remain missing; storage boot scenarios do not exercise them.

Correction 2026-10-16: prior checkmarks and cited standalone-model/storage-boot receipts did not establish these criteria. Acceptance remains pending; historical observations are not runtime qualification.

## Risk / rollback and assumptions

Availability yields to authority/fences and durable budgets. No federation, full consensus, timeout promotion, multiwriter, DSM or transparent migration. Remote effects/epochs/ledger transactions are irreversible by binary revert: close admission, preserve outcomes/unknown charges and reconcile under policy. Public RemoteEndpoint currently refuses; fixtures/proposed service paths are not implementation evidence. Missing protected Intel/physical prerequisites leave profiles disabled and gates unpassed. Ten pending phases, unchanged dependencies, 38 locks and checkpoint/Intel gates remain intact.
