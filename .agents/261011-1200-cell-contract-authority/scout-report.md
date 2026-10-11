# Recon — Cell / Kernel / authority cutover

**Date:** 2026-10-11. Read-only code research plus existing-plan inventory; no new runtime behavior proved by this document. Parent reused the prior architecture review and read current contracts; two read-only scouts examined thread/SMP and distributed authority. Separate plan-lock owner reconciles historical statuses in the lock register.

## Source-confirmed findings

| Boundary | Evidence | Consequence |
|---|---|---|
| Thread grouping | `kernel/src/task/tcb.rs:218-224,479-493`; `scheduler.rs:735-825` | Reuse CellId/generation/root grouping; threads are not new principals. |
| Task capability snapshots | `kernel/src/task/scheduler.rs:754-789,823-826` | Shared mutable CSpace replaces stale copied authority, not just renamed fields. |
| Capability owner lacks incarnation | `kernel/src/cell/cap_registry.rs:35-46,113-145` | Bind issuer/holder lifetime to generation before handle reuse. |
| Root retirement | `kernel/src/task/scheduler.rs:1146-1190,1511-1574`; `kernel/src/task.rs:1362-1408` | Preserve live→retiring, all-member quiescence and post-ack resource release. |
| SMP ordering | `kernel/src/task/hart_local/ready.rs:141-177,231-263`; `task/smp.rs:196-243` | selected/current/executing/outgoing ownership is load-bearing. |
| Operation ownership | `kernel/src/task/async_ipc.rs:21-43,179-276` | Caller operations task-local; explicit reply accepted from same provider Cell incarnation. |
| Completion queue | `kernel/src/task/completion.rs:11-26,280-325` | Reservation/deferred wake useful; one waiter is not multi-thread wait support. |
| TLS/futex | `kernel/src/task/tls.rs:1-118`; `futex.rs:1-30,136-220` | Preserve neutral per-thread context and atomic domain-scoped wait, not POSIX policy. |
| CPU budget | `kernel/src/task/tcb.rs:414-435`; `scheduler.rs:1908-2020` | Per-task ticks/watchdog are not aggregate Cell CPU quota. |
| Launch policy | `kernel/src/loader/launch_profile/mod.rs:54-90`; `governed_spawn.rs:214-244` | Exact reviewed edges currently protect authority; remove only after equivalent capability/artifact enforcement. |
| Native POSIX leakage | `libs/api/src/services.rs:25`; `services/posix.rs:8-46`; `third_party/mlibc/sysdeps/vicell/generic/generic.cpp` | Explicit runtime selection and consumer migration required. |
| Bootstrap FS | `kernel/src/fs.rs:11-18,100-112` | Legacy fd removal must not break VFS/loader bootstrap cycle. |
| Raw ring | `libs/ostd/src/ring_channel.rs:141-148,154-206` | Pointer/alignment and next-reply behavior is not a general capability/lifecycle contract. |
| Remote route absent | `libs/ostd/src/cluster_endpoint.rs:191-207`; `cells/services/net-broker/src/local_runtime/request_dispatch.rs` | Public remote call refuses; envelope/ingress modules are non-activating foundations. |
| Remote ingress | `cells/services/net-broker/src/c2c_ingress.rs`; `c2c_dedup.rs` | Need real protected/live proof producers, bounded transport ownership, durable reconciliation beyond retained dedup. |
| VFS authority | `docs/specs/09c-vfs-directory-capabilities-adr.md:13-17,91-122` | Preserve service authority, transitive revoke and reacquire-on-restart; post-start receive is an explicit amendment. |

## Precedent history inspected

`git log` on architecture/IPC authority paths returned:
- `240ee023b` — deterministic two-hart publication and wake race tests.
- `c88008d8f` — synchronous/bounded RPC cutover to kernel-owned operations.
- `783d7eb65` — local service lookup bound to provider generation.
- `627c76b5a` — bounded asynchronous IPC operations and queueing send.
- `efb0ce7eb` — Intel x86-64 sole C2C direction.

These subjects identify precedent; they do not prove new contract behavior. Current code/phase evidence remains authoritative over stale narrative snapshots.

## Reused runtime evidence ceiling

Earlier in this conversation an existing `build/vicell-x86-c2c-lifecycle.iso` was booted in QEMU and the local lifecycle command reached `[local-lifecycle] PASS`, including PeerGone, saturation, pre/post-dispatch timeout and replacement. Artifact SHA-256: `b1cdb20d519c145591ad05de3b6a22bf62259ed6475e152a0ca41d464310343a`. It was not rebuilt for this planning task. This is existing-artifact local behavior only, not source-qualified new authority, real remote C2C, physical Intel or whole-state recovery evidence.

## Earlier design adjudications from research

- Retain same-incarnation explicit reply tokens across sibling workers; reject a new per-thread security/delegation layer because same-Cell threads share memory and trust. Still require captured tokens for deferred work and reject foreign generations.
- Choose whole-Cell retirement for unhandled shared-memory-threatening worker faults, although current worker exit/fault handling can be task-local. The stronger safety policy is a target semantic amendment, not a claim of current behavior. Clean worker exit remains local.
- Preserve service-owned resource authority and typed handles, not one universal kernel ACL copy. Shared kernel CSpace applies to kernel/transport authority; issuer resource tables remain the semantic source.
- Preserve native stream mechanism when needed for enforcement. POSIX-shaped names alone do not justify kernel removal.
- Select fail-closed mutable remote admission/commit fencing rather than pretending distributed revoke is instantaneous; two-node software proof does not qualify physical/production authority.

## Approved proposed change — not source recon or runtime evidence

Placement here means one application across selected nodes within the admitted cluster; it does not mean inter-cluster federation or a new trust domain.

The maintainer subsequently approved the following documentation-only target. These are proposed implementation requirements, not findings that current code implements them; the source-confirmed table and reused artifact ceiling above are unchanged.

- One Cell belongs to one node (one physical/VM kernel instance); cluster means multiple nodes. Hundreds of concurrent clients/requests are served by bounded worker pools/operations, not implicitly hundreds of Cells. No DSM or transparent thread migration.
- Broker Cell owns peer sessions, reconnect, direct/relay choice and a bounded pool; service-net owns IP/TCP; protected authority owns trust and relay TLS; kernel owns only local primitives. C2C hides transport but retains explicit timeout, refusal, disconnect and uncertain-effect semantics.
- P08 separates control and request data planes: fixed-home fenced placement-controller Cell owns desired replicas/application ledger/versioned route publication; per-ingress workload-router Cells own selection/queue and exact original-subject typed authorization propagation. Future `cells/services/placement-controller/src/{policy.rs,budget_ledger.rs,placement.rs,routing.rs,scaling.rs,reconcile.rs}` and `cells/services/workload-router/src/{endpoint.rs,admission.rs,routing.rs,queue.rs,telemetry.rs}` are proposed targets, **not existing code**.
- Logical pools apply only to explicitly replication-compatible typed methods; existing concrete generation-pinned endpoints keep their semantics. Stateless replicas may balance; shard/writer/session affinity stays on the approved ownership map, without reshard/writer cloning. Destination issuer/state fence authorizes effects, supervisor owns lifecycle, broker only transport/session; router is not ambient authority.
- Bounded weighted slot-cycle routing uses stable replica ordering and GCD-normalized positive integer weights derived from concurrency allocation, with both member/slot limits. Per-router/per-replica shares and authoritative destination caps gate selection; draining/suspect/stale/unauthorized targets are ineligible. One selected attempt freezes the concrete target; refusal or ambiguous result is returned without retry/hedge/replay. Local preference is bounded explicit weight only.
- Durable atomic application reservation precedes spawn; node admission may independently refuse. Nested workload/node/router shares and router/controller overhead are charged, without per-request durable transactions or automatic borrowing. Live/reserved/unknown/draining charges remain accounted; delayed key publication must be fenced before reclaim. Below-use budget shrink is pending, never forced eviction, and resize reserves positive delta first.
- Explicit policy includes allowed destination/artifact/tier/architecture/state-locality/authority filters, concurrency weights/shares, finite resource vectors and min/max/rate/pending/queue/cold-start bounds. Placement ranks least application-charged replicas per node, normalized headroom then NodeId, using fresh capacity hints with destination-authoritative refusal/reconcile. No exact free-resource promise.
- Scaling samples authorized ingress queued+in-flight request counts exactly once with source epoch/sequence/window/local receive age. CPU budget is execution-time entitlement per common period; queue/memory quantities use bytes and slots/counts, not throughput/MHz. Complete fresh samples compute ceil(demand/targetConcurrencyPerReplica) under min/max/hysteresis/cooldown/rate/budget gates. Missing samples freeze, not scale to zero; min-zero requires already charged always-on ingress/control and bounded coalesced wake.
- [Spec 26](../../docs/specs/26-workload-placement-scaling-contract.md) details this normative extension under Spec 25 without new acceptance numbering. AC-01..AC-35 remain; AC-36..AC-42 belong to P08 and AC-43 to P10. P01 inventories exact metadata/symbol/caller migrations. P08's true internal pending order A transport → C budget/placement → B routing/admission → D autoscale/reconcile → E failures preserves the ten-phase DAG. C supplies durable controller budget, real ingress shares and versioned bootstrap membership before B admits live routing; no mock-grant substitute.
- P08 oracle traces cover unequal weights/caps/affinity, sample windows/stale data, cold-start expiry, delayed spawn/lost reply, drain race and pending budget shrink. P10 must observe actual demand-driven remote worker spawn and measured balanced multi-node work, not merely 200 clients or manually provisioned replicas. Controller outage allows only still-valid routing AND live destination authority; no budget/placement mutation or timeout promotion.
- All ten phases remain pending and all 38 reciprocal locks remain in force. Architecture approval is not ABI ratification, protected Build/activation or physical/production approval, and adds no runtime proof. Relay-only proof precedes separately qualified direct LAN. Source-confirmed findings and historical evidence above are unchanged.


## Research limitations

Semantic `find` failed with tool-provider HTTP 402 during inventory; literal/filename reads supplied the evidence instead. That is a tooling failure, not a Cellos runtime defect. Existing LSP previously failed in this session; actual implementation must retry availability and use symbol references when available. No new tests/builds/formatters were run by scouts. Linux comparison in ADR-0024 is semantic fit, not a measured security/performance win.
