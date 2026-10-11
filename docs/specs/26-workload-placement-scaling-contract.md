# Spec 26 — Workload placement, load balancing and autoscaling

**Date:** 2026-10-11
**Version:** 1.0
**Status:** Selected architecture under the maintainer's request to settle design before implementation. No runtime implementation, exact ABI or production activation approval is implied.
**Authority:** [ADR-0024](../decisions/0024-cell-kernel-authority-contract.md); normative extension of [Spec 25](25-cell-kernel-authority-contract.md). Acceptance IDs are owned only by Spec 25, AC-30..AC-43; do not duplicate a competing acceptance ledger.
**Execution:** [controlling plan](../../.agents/261011-1200-cell-contract-authority/plan.md), P01/P08/P10. [Existing scoped locks](../../.agents/cell-contract-0024-locks.md) remain effective.

## 1. Scope and selected architecture

One Cell incarnation executes on one node. A workload is a declared set of compatible service replicas for one application across selected nodes within one admitted cluster. A logical workload endpoint is not a distributed Cell or shared address space. Hundreds of concurrent requests do not require hundreds of Cells or connections.

Placement chooses where a replica is created. Load balancing chooses an existing eligible replica for a new request. Autoscaling changes desired replica count from bounded observations. These are separate decisions: a saturated request does not itself authorize spawn, remote fallback or replay.

| Owner | Contract responsibility | Forbidden responsibility |
|---|---|---|
| Fixed-home placement/autoscaling controller Cell | Desired state, policy revision, durable application reservations, lifecycle orchestration and versioned route publication | Per-request global scheduling; authority promotion after timeout |
| Workload router Cell on each admitted ingress node | Logical endpoint admission, bounded local queue, replica selection and correlated completion | Mint caller rights, create replicas or silently retry effects |
| Supervisor Cell at destination | Scoped keyed prepare/restore/ready/retire and authoritative lifecycle reconciliation | Override local admission or issue application budget |
| net-broker Cell | Authenticated peer sessions, bounded multiplexing, reconnect and qualified direct/relay transport | Workload scaling policy or treating a connection as method authority |
| Resource/state authority | Method/resource authorization, committed outcomes and writer fencing | Allow a second writer just because a replica was added |
| Kernel Cell and service-net | Local isolation/accounting/IPC; IP/TCP owned by service-net | Cluster placement policy or globally shared kernel scheduling |

Controller/ledger and ingress routers bootstrap through explicit supervisor grants and reserved control-plane capacity, independently of the replicas they manage. Their envelopes are charged to the application. The controller is not on every request path. Router count and per-node deployment are explicit and bounded; this is not a new router Cell per request or an unlimited automatic mesh.

## 2. Workload definition and logical endpoint

P01 SHALL inventory an exact public/persistent representation of these semantic fields before any ABI change:

| Group | Required meaning |
|---|---|
| Identity | Application/workload identity, admitted cluster, policy revision, controller epoch |
| Service | Artifact/template allowlist, service/schema version, method set and replication/affinity classification |
| Placement | Eligible nodes, execution tier/architecture, required resource/state locality, per-replica resource vector |
| Budget | Application vector, router/controller shares, node/replica ceilings, queued bytes/operations, subject limits |
| Routing | Bounded member count, weight/concurrency shares, route validity, health freshness, affinity map revision |
| Scaling | min/max replicas, target concurrency, sample interval/age, consecutive up/down windows, cooldown, bounded step/rate/pending limits |
| Queue/lifecycle | Queue capacity, request/queue deadlines, preparation timeout, reconciliation bounds, drain policy, explicit min-zero opt-in |

Values SHALL be finite, dimensionally valid and authority-bound. Missing/invalid policy is refused; min replicas is desired availability, never permission to exceed budget. Policy changes use monotonic revisions and exact owner grants; receiving a name, route table or metrics record grants no authority.

A logical workload endpoint SHALL be explicitly requested by a caller authorized for that pool's service/method/export scope. Existing concrete generation-bound endpoints remain pinned; they are not silently reinterpreted as balanced endpoints. Only replicas with the same declared method/schema and effect semantics are interchangeable. Local and remote replicas share the operation contract, not a pointer ABI or equal latency/failure behavior.

The router SHALL retain kernel-attested original caller identity or an issuer-approved constrained delegation through each hop. The destination verifies audience, subject, method/resource, node/incarnation and export rights. A router cannot spend broad service rights on behalf of an arbitrary caller. Selection resolves an authorized logical pool to one concrete provider generation and attempt binding before submission; the selected binding is immutable thereafter. This resolution does not change the request's identity, payload, deadline or permitted effects.

Stateless methods may use any eligible replica. Stateful shard/session-affine methods use an explicitly versioned ownership map and route only to the authorized shard/writer/session owner. An unavailable owner is not permission to reroute to an independent copy. Automatic resharding, session migration and multi-writer replication are outside this contract; a planned writer transition follows Spec 25's watermark/fence protocol.

## 3. Routing, backpressure and request lifetime

Baseline selection is **bounded weighted round-robin** over authorized Ready members. Positive integer weights derive from admitted concurrency allocations, not untrusted instantaneous load. Each ingress has explicit per-replica in-flight shares; their total SHALL fit the replica's declared concurrency envelope. Destination admission remains authoritative under races, stale observations or failed routers. Shares are not duplicated when a router restarts; the old ingress generation must be fenced and retained requests reconciled before reuse.

For an exact oracle, order replicas by stable replica identity, divide integer weights by their common greatest divisor, and precompute a slot cycle repeating each identity by that normalized weight. A weight pair A:2/B:1 yields A,A,B. Both member count and total cycle slots have explicit profile bounds; oversized policy is refused, never allocated on the request path. The cursor advances after every inspected slot (including skipped entries), wraps, and inspects at most one cycle for each decision. Selection atomically reserves the local in-flight share; a locally lost reservation race continues only within that inspection bound before any submission. A new route revision rebuilds the bounded cycle and resets its cursor; this does not change already submitted bindings. Turn-sequence claims apply per router under a fixed revision and declared eligibility/capacity.

Per-ingress concurrency shares are reserved suballocations of a replica envelope. Expanding the ingress set or moving a share requires controller-approved reallocation: fence old-generation admission and reconcile its retained requests before the new share can be used. A restarted router cannot reset counters to zero while its prior generation may still have dispatched work. This intentionally trades some utilization for a bounded, non-oversubscribing baseline; automatic borrowing of unused ingress shares is not implied.

The candidate set excludes Preparing, Draining, Retired, unauthorized, incompatible, unhealthy or expired members and members with no remaining local share. The cursor visits at most the configured slot-cycle bound; it does not scan the cluster, allocate per candidate or contact the controller for each request. Route tables and counters are preallocated within the router envelope. Explicit bounded weight can express locality preference; local placement is not an unconditional rule that starves available remote capacity. Equal admitted members without exclusions receive equal turns; general equal throughput is not promised.

Selection and pre-dispatch queue ownership are linearized in the router. If no eligible member has capacity, a request may wait only within its admitted FIFO queue, subject limits, byte/operation reservation and remaining deadline. Queue residence consumes the original budget. No queue capacity means an explicit pre-dispatch overload/refusal; no eligible route means bounded wait or unavailability, never an unbounded backlog. Long-running streams count as active load and retain their resources; scale-down cannot move their live state.

There is **one selected submit attempt per request** in the baseline. Destination pre-dispatch refusal is returned as such, not hidden by a retry to another replica. A later independent caller request may select another member under the same policy. Post-dispatch cancellation, disconnect, timeout or reply loss remains uncertain where required; reconnect, rebalancing and autoscaling cannot hedge or replay it. Queue-expired requests are removed and cannot execute when a cold replica becomes Ready later.

Readiness requires initialized/restored service, admitted resources, live export and state fence where applicable; a TCP connection or process heartbeat alone is insufficient. Health loss suppresses routing but is not proof that the Cell is gone or its budget is reclaimable. Publication uses controller/policy revision plus exact node/boot/Cell/export generation. Receiver freshness uses bounded local monotonic age and admitted epoch/sequence; packets do not acquire trust from timestamps or reset an absolute origin deadline.

Duplicate or lower-sequence route/health observations cannot renew validity. A route revision change is accepted only from the admitted publication authority and cannot widen the original invocation grant; expiry alone never revokes or releases a running operation's retained lifetime.

Removing a route is not enough to drain: the destination SHALL close the selected replica's new-admission gate and acknowledge the cutoff. A stale router arriving after that cutoff is refused before effect. Work admitted before it remains tracked and drains under the original operation contract. Only after resource/state quiescence and confirmed retirement may the controller release charge; a partitioned drain remains charged/pending.

## 4. Application-wide budget and node placement

Represent budget as a declared vector B. For each dimension, sum of control/router shares and all replica reservations (preparing, live, draining, in-flight and unknown) SHALL be at most B. A ledger entry owns each charge exactly once; nested per-router/replica sublimits narrow the envelope rather than grant another copy. Memory includes reserved stacks and metadata; operation counts/queued bytes are finite. CPU is execution entitlement over a common declared accounting period, not MHz, instantaneous idle percentage or a cross-machine throughput equivalence.

Local and remote spawn both require an atomic durable reservation before submission. The destination checks reservation application, destination, artifact/tier/resource ceiling, stable creation key and controller epoch; a reservation cannot fund two children or be reused at another destination. Delayed old requests cannot publish after cancellation/release: release requires confirmed absence/Gone plus durable key closure/fencing. Existing descendants cannot bypass the application ledger via ungoverned child-spawn grants. Shared broker/NET overhead is assigned to explicit bounded node/service infrastructure envelopes, with attributable workload queues charged to the application; it is not an unlimited unaccounted per-request side channel.

A replica resize first reserves its positive budget delta and obtains destination acceptance before widening its local limits. Failure or uncertain acknowledgement retains the safe charge and reconciles; no transient oversubscription. Reducing application budget below existing charge records a pending target, closes new growth and drains safely; it is not applied as an already-satisfied lower ceiling or a license to kill admitted work. New budget/capacity grants require the owning authority, not an autoscaler decision.

Placement first filters nodes by explicit grant, artifact/tier/architecture support, resource/state locality and fresh authenticated capacity observations. Rank feasible nodes by lowest charged replica count for this application, then largest normalized post-reservation headroom in the limiting declared resource dimension, then stable NodeId. P01 fixes resource units/normalization and bounded candidate count; hard constraints always outrank scores. Stale/unknown capacity is not positive free capacity. The destination atomically reserves its own resources or refuses; source observations cannot guarantee destination acceptance.

Concurrent applications are arbitrated by destination admission, not a global free-resource assertion. A refused create can lead to a later bounded placement decision only after its reservation/key is safely reconciled; never reuse an ambiguous create on a different node. Changing placement policy affects future decisions; it does not imply live migration or evict healthy replicas merely to equalize a score.

## 5. Autoscaling control loop

Only authenticated, authorized demand within admitted queue/subject bounds feeds scaling. Each request contributes once at its owning ingress from accepted queueing through terminal reconciliation, including work dispatched to a replica; do not also count the worker copy. Ingress reports contain workload identity, source incarnation, monotonic sample sequence, queued/in-flight counts and admitted capacity. Source restart replaces, not adds to, that source's previous sample. Missing/stale/invalid sources are not zero load; publication and receipt-age limits bound the observation window without claiming a simultaneous globally synchronized snapshot.

At each declared sample interval, D is the sum of latest valid nonoverlapping ingress demand. T is the configured positive target concurrency per replica for the qualified workload profile. Compute desired = clamp(ceil(D / T), minReplicas, maxReplicas). T is measured/tuned configuration, not a scheduler guarantee. Scaling up or down requires its declared consecutive-window threshold, hysteresis and cooldown; down thresholds are stricter than up thresholds to prevent oscillation. Integer rounding, counter overflow and sample bounds are explicit refusal conditions, not unbounded growth.

Each decision then applies per-step/rate limits, application budget, destination admission and pending-create bounds. Count Preparing and Ready replicas once when calculating additions; Draining/unknown instances remain budget/max-instance charges but are not usable Ready capacity. Reconcile unknown creates before new scaling mutations. Missing required telemetry freezes scaling at current desired state; it cannot trigger scale-down. An infeasible minimum or target is reported unsatisfied with a reason (budget, capacity, authority, telemetry, preparation or state); queued callers retain normal deadlines and overload outcomes.

The controller keeps a bounded durable decision/intent identity linking policy revision and desired count to reservations and creation keys. Repeated observations/restarts reconcile that intent instead of creating duplicate children. Preparation failure never publishes a route. Preparation timeout without authoritative absence is unknown, not free budget. Backoff/reconciliation is bounded by policy; failed/slow starts cannot cause unlimited replacement attempts.

Scale-to-zero is allowed only by explicit min-zero policy with reserved always-on controller and ingress-router capacity. An authorized cold request may enqueue one bounded wake signal per workload; the controller coalesces concurrent wakes into one initial creation intent, then the ordinary bounded loop may add capacity. No cold request performs spawn directly. Cold-start wait remains within queue/deadline bounds; expired requests are not replayed. With no valid cold-start control path, min-zero policy is refused rather than leaving a permanently unreachable service.

Scale-down selects eligible replicas without violating pinned state ownership, removes routes, closes destination admission, drains, retires and releases proven charge in that order. All slots remain counted until confirmed retirement. Administrative stop/fault recovery stays distinct from autoscaling; a lower desired count never authorizes killing admitted work or losing required state.

## 6. Controller failure and availability boundary

The controller/ledger has one assigned home and a fenced epoch. Restart recovers durable reservations/intents before new mutations; inability to prove the ledger/epoch means no placement, budget grants, resizing or scaling. Timeout/partition does not promote another controller or state authority. General consensus and controller-home failover remain separately scoped work.

Controller outage need not terminate healthy replicas. Existing routers may serve within already reserved envelopes only while route publication validity, original caller/export authority and destination live admission/state fencing remain provable. A cached route is never cached authorization. Expired membership or unreachable authority required for a mutable method yields unavailability; this does not introduce disconnected read leases. No controller outage allows pool expansion or reservation release from stale inventory.

## 7. Delivery slices and qualification

P01 freezes exact fields, units, symbols, policy bounds and ownership migration before code. P08 proceeds through transport proof, application budget/placement and authoritative membership, workload routing/admission, autoscaling/reconciliation and combined failure qualification; no slice claims the later ones are already working. Budget/controller bootstrap precedes live routers so no fixture grant or alternative publisher is required. P10 runs at least 200 actually overlapping clients locally and across admitted cluster replicas, plus the following observable scenarios:

- Unequal weights and saturated member shares yield the configured turn sequence among eligible members; incompatible/unauthorized/affine peers never receive the request. Concurrent routers cannot exceed the destination envelope.
- Increasing admitted demand creates additional budgeted replicas on eligible nodes, publishes them only at Ready, and sends new requests to them; falling demand drains and retires safely. Merely pre-spawning replicas is not autoscaling evidence.
- Trace demand samples, formula, hysteresis, cooldown, rate limits and charged reservations against independently computed expected decisions. Duplicate/stale/missing samples do not create oscillation, extra charge or unsafe downscale.
- Node saturation, application exhaustion/shrink and slow startup return bounded overload/unsatisfied targets; no unlimited queue, child count or implicit replay.
- Burst cold-start coalesces; requests expiring before Ready never execute. Lost create reply, each-boundary controller crash, delayed create after release, and router restart reconcile without duplicate child/budget.
- Race route removal with new admissions and partition; destination cutoff rejects stale routers, admitted work/state remains accounted, fixed-home authority is not promoted.

All are pending implementation and runtime evidence. No load-balance fairness/latency/availability promise extends beyond the declared policy and tested failure profile. Keep relay-first, separately qualified direct LAN, exact ABI checkpoints and physical/production evidence ceilings from Spec 25.

## Change log

- v1.0 — 2026-10-11: explicit architecture-first request expands the approved placement scope into a complete workload endpoint, routing, budgeting and autoscaling contract; implementation deferred. Canonical acceptance remains Spec 25 AC-30..AC-43.
