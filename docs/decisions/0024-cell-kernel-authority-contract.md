# ADR-0024 — Cell, Kernel Cell and object-capability authority

**Date:** 2026-10-11
**Status:** Accepted architectural direction and contract baseline, including the approved placement/autoscaling amendment; runtime implementation remains pending.
**Contract revision:** Spec 25 v1.3 / Spec 26 v1.1, including the maintainer-approved five-group red-team amendment and independent Budget Authority choice.
**Decider:** Cellos maintainer, explicitly approving the Cell-centric proposal, the follow-on placement/autoscaling controller Cell and application-wide budget, and requesting the contract, detailed plan, multithreading/distribution review and suspension of overlapping unfinished plans.
**Implementation authority:** No new syscall, wire layout, method discriminant, manifest field, remote export or production admission is approved here. Each concrete public delta requires the existing Law-1 checkpoints on its exact revision. This request is not a checkpoint for an unspecified ABI.

## Context and drivers

Cellos has generation-attested principals, bounded RPC operations, directory capabilities, user-space supervisors and execution tiers. It still has per-task capability snapshots, name/path-selected launch policy, multiple completion lifecycles and ambient POSIX linkage. Calling these all Cells without unifying lifetime and authority would preserve those contradictions.

Drivers: least authority by construction; safe multicore threads; explicit local/distributed failure; bounded hot-path cost; restart without restarting dependents; retention of existing safe code and frozen ABI until approved cutover. The sole program remains [Intel x86-64 C2C (ADR-0022)](0022-intel-x86-64-c2c-only-direction.md).

## Decision

1. **Cell is the principal and recovery boundary, resident on one node.** A node is one kernel instance on physical hardware or in a VM; a cluster comprises multiple nodes. Role (app/service/driver/supervisor) and isolation tier are separate axes. Threads are node-local execution contexts of one Cell incarnation, not new principals. Libraries, arbitrary data objects and individual requests are not independently scheduled Cells; hundreds of concurrent requests do not require hundreds of Cells.
2. **Kernel Cell is the first, exceptional root principal.** Pre-scheduler initialization is its bootstrap stage. It retains isolation, scheduling, capability integrity, atomic publication/retirement and bounded fault capture. Boot delegates scoped authority to init and supervisors. Kernel failure requires node recovery, not a promise of ordinary Cell restart. Internal kernel functions need not become queued RPCs.
3. **Use typed object capabilities, not ambient UID/root permission.** One shared, kernel-protected capability space per Cell incarnation; opaque generational handles, rights attenuation, explicit transfer/delegation, bounded derivation and transitive revocation. No path, service name, signature, tier, TID or remote node authentication alone grants operation authority. Artifact provenance and launch resource ceilings remain mandatory.
4. **Resource authorities stay with their owners.** Kernel owns kernel objects and authenticates invocation/transfer. VFS owns directory/file semantics; service-net owns IP/TCP and network-resource semantics. A broker Cell owns peer sessions, reconnect policy, direct-versus-relay selection and a bounded reusable connection pool; protected authority owns peer trust and protected relay admission. A universal capability *contract* does not mean a duplicate kernel copy of every service's resource ACL. Service-specific handles remain typed. A broker cannot turn an arbitrary integer or claimed identity into authority or substitute broker-owned TLS for the protected authority endpoint.
5. **One C2C operation contract, multiple transports.** Reserve terminal capacity before admission, bind caller/provider incarnations, correlate exact operations, distinguish pre-dispatch refusal/cancel from uncertain post-dispatch outcome. Copied IPC is the default; grants/rings are authorized local data transports. Remote marshals data, never raw pointers. C2C hides transport selection, not failure semantics; no transparent local-to-remote fallback.
6. **Separate committed business state from execution.** Spawn and replacement share prepare/restore/ready/publish semantics. Resource rights are reissued to the new incarnation, never restored from opaque serialized handles. Crash records precede reclaim. Exactly-once side effects require a transactional application authority and are not a transport guarantee.
7. **POSIX is optional compatibility outside the native core.** Local C/C++ ABI/runtime state stays in the client; domain-selected personality Cells implement only required process semantics over native services. Native boot and native applications do not depend on a universal POSIX broker. TLS, wait/wake and bounded stream mechanisms may remain kernel-owned when justified by Spec 15; syscall names alone do not determine the boundary.
8. **Enforcement, policy and routing have separate owners.** Independent fixed-home Budget Authority Cell owns durable finite application reservations, issuer-precommitted genesis and live destination claims; controller owns desired replicas, placement/scaling policy and versioned membership but cannot mint allowance. Routers own bounded queues and opt-in selection without global per-request transactions. Destination Ready evidence and immutable origin proof with issuer-sealed single target preserve authority. Broker, supervisor and resource/state issuers retain transport, lifecycle and effect ownership.

Precise semantics and independently observable requirements are in [Spec 25](../specs/25-cell-kernel-authority-contract.md); [Spec 26](../specs/26-workload-placement-scaling-contract.md) is its normative detailed placement, workload-routing, application-budget and autoscaling extension, not a separate acceptance ledger. Implementation is owned by [the cutover plan](../../.agents/261011-1200-cell-contract-authority/plan.md).

## Why this authority model, compared with Linux

The comparator is traditional ambient Unix UID/GID/DAC plus broad superuser privilege, not a claim that modern Linux has only that model. Linux also has per-thread capabilities, user namespaces, seccomp, LSMs and Landlock; file descriptors already provide useful object-reference semantics.

| Choice | Advantage | Cost / decision |
|---|---|---|
| UID/GID + root + global path policy | Familiar administration and compatibility | Authority follows ambient identity, broad classes of operation and naming. Reject as native Cell model; retain only inside POSIX personality if needed. |
| Linux-style privilege bits + namespaces + optional sandbox policy | Incremental migration from CapSet; mature precedent | Still requires composing multiple policy systems and does not directly describe per-object delegation/restart. Reject as target; keep existing checks until complete capability replacement. |
| Globally valid signed bearer tokens for every local/remote operation | Uniform serialization | Replay/leak exposure, expiry/revocation partition tradeoffs and needless local crypto. Reject as local hot path and as unconstrained remote authority. |
| Typed object capabilities + immutable admission ceiling + explicit receive/delegate policy | Least authority, scoped revocation, explicit resource and incarnation ownership | Chosen. Requires a capability table, derivation accounting, service enforcement and careful SMP reclamation. No claim of superior measured speed or universal security. |

Local invocation uses bounded table lookup and preallocated/slab-backed records, not cryptographic signature verification or heap allocation per call. Remote export grants are separately authenticated, audience/subject-bound and revocable at the destination. Phase 10 compares the same scoped workload with Linux DAC and a documented hardened configuration; measurements, not this ADR, establish performance.

## Multithreading and distribution fitness

- Shared Cell CSpace and aggregate quotas prevent thread creation from multiplying authority or budgets. Per-thread TLS, stacks/registers and saved reply contexts remain separate. Same-incarnation workers can finish delegated reply work; token possession alone does not authorize a foreign Cell.
- Root exit and any unhandled memory-safety-threatening worker fault retire the whole Cell; clean worker exit retires only its thread. No service owner outlives its retiring root. All CPUs acknowledge departure from the old execution/mapping domain before reuse.
- Remote node, boot/session, Cell, service and object generations are distinct. Authenticated transport proves a peer node, not an end user's right to a method. Delegation is explicit and bounded.
- Partition policy is fail-closed for new mutable operations requiring current authority/fencing. No new remote invocation may reuse an authorization decision cached before the partition. Already dispatched effects may finish and remain indeterminate; cancellation/revocation cannot undo them. Availability is deliberately sacrificed where authority cannot be established.
- General distributed kernel scheduling, transparent thread migration, distributed shared memory and general consensus are not prerequisites or implied features. The approved controller Cell performs explicit replica placement/autoscaling across selected admitted nodes within one cluster; threads remain node-local. A partition-safe mutable service requires an authoritative fencing/commit boundary, not two competing supervisors or automatic state-authority promotion.

## Compatibility, precedence and amendments

This ADR selects a target contract, not an implementation claim or permission to reinterpret frozen bytes.

| Existing decision | Retained | Required target amendment / migration gate |
|---|---|---|
| Spec 15 | Minimal privileged substrate; kernel enforcement cannot move into untrusted policy | Clarify Kernel Cell identity and shared CSpace; replace CapSet implementation wording only after equivalent enforcement exists. |
| ADR-0015 / Spec 22 | Tier 1 trusted SAS; Tier 2 hardware isolation; Tier 3 guest; fail-closed admission | Raw-address rings are not capabilities; optimization only after lifetime proof. Do not move FFI into SAS. |
| ADR-0018 | Userspace POSIX, runtime profiles, TLS/futex primitives | Explicit optional runtime linking instead of ambient `api` POSIX. Retain or replace kernel stream mechanism based on boundary proof, not a blanket deletion. |
| VFS directory-capability decision | Typed handles, transitive revoke, no handle inheritance across replacement; VFS owns semantics | Spawn-time grants remain default. Post-start receipt is allowed only within a fixed admission ceiling and an explicit receive policy; this changes the old spawn-only design and needs exact VFS/ABI amendment before implementation. |
| ADR-0023 / Spec 17 | Existing Cell generation binding, bounded operations, frozen LookupServiceBound | No reuse of IDs/wire slots. New shared ownership/waitset/capability operations require their own Law-1 approvals. |
| ADR-0008/0009 | Authority-owned protected relay TLS endpoint; correlated fail-closed failures | No broker-owned TLS substitute, raw-record fallback, or relaxed AC-012 gate. |
| ADR-0021 | Userspace supervision policy | Whole-Cell fault scope and scoped lifecycle capabilities amend implementation semantics only through Phase 01 gates. |

### Approved follow-on amendment

The maintainer approved scoped placement/autoscaling and subsequently selected independent Budget Authority enforcement rather than trusting the policy controller's ledger. Authority and admitted destination kernel form the budget TCB; controller compromise cannot mint distinct-key allowances beyond the application ceiling. Genesis is issuer-precommitted before control spawn; supervisor consumes a one-use keyed grant. Destination live atomic claims forbid offline copied allowance. Authority outage sacrifices new placement availability; existing admitted work remains subject to its reserved bounds and live effect authority. No failover or timeout promotion is inferred.

P08 owns the extension and P10 qualifies all AC-01..AC-50. The v1.3 checkpoint explicitly amends affected earlier semantics and appends AC-44..AC-50. Bounded receiver-start challenges replace arrival-age freshness. Old ingress generations remain required until authoritative reconciliation/fencing; automatic demand/T is restricted to homogeneous interchangeable domains. Affine unmet demand cannot spawn an ineligible sibling.

Destination-issued Ready evidence binds the application/artifact/tier/reservation/create key/export/schema/generation. Origin issuer binds canonical request/deadline/operation and seals at most one concrete target. Immutable incarnation maxima constrain mutable admitted limits; growth reserves its delta and excess growth needs explicit replacement. Unknown children remain charged until terminal key closure. These decisions approve architecture only, not ABI bytes, implementation or runtime qualification.

Node CPU budgets express admission/enforcement limits, not MHz, performance or a benchmark promise of request capacity. Relay-first operation and direct-LAN qualification remain governed by existing authority, locality and evidence gates; controller/router policy cannot bypass them. CELL-CONTRACT-0024 and all 38 existing lock dispositions remain unchanged.


## Execution freeze

Lock **CELL-CONTRACT-0024** suspends overlapping unfinished work; it does not erase completed evidence. [Lock register](../../.agents/cell-contract-0024-locks.md) records the plan-by-plan disposition; [portfolio](../../.agents/plan-portfolio.md) and [current focus](../roadmap/current-focus.md) are scheduling entrypoints.

No old plan resumes automatically when these documents exist or a phase is complete. The maintainer must admit a specifically rebased slice, with exact file ownership, contract/ABI gates and evidence ceiling. Necessary unfinished safety work is adopted into new phases, not a dependency cycle requiring the old locked plan to run. WIP remains one implementation slice.

## Consequences

- Positive: one principal/authority/lifecycle vocabulary across kernel, drivers, services, applications and remote adapters; stronger least-authority default than ambient-root design.
- Costs: table/derivation storage, revocation coordination, migration of every caller, explicit service recovery, durable application-budget reservation/reconciliation and remote availability tradeoffs; fixed-home authority failure can stop placement/scaling without losing reservation ownership.
- Limits: capabilities do not prevent an authorized malicious service from abusing its own resource, cure unsafe SAS code, provide confidentiality by themselves, or recover a corrupted kernel. Isolation, service validation and existing signing/admission remain load-bearing.
- Rollback: before authority/state cutover, restore a known-safe image and cold boot. After persistent state/export-epoch changes, binary downgrade alone is unsafe; deny old formats/epochs and recover from an approved backup or forward migration. Never reopen old insecure gates as rollback.

## Sources

- [seL4 capability tutorial](https://docs.sel4.systems/Tutorials/capabilities.html), [scheduling contexts](https://docs.sel4.systems/Tutorials/mcs.html).
- [Linux capabilities](https://man7.org/linux/man-pages/man7/capabilities.7.html), [user namespaces](https://man7.org/linux/man-pages/man7/user_namespaces.7.html), [Landlock](https://docs.kernel.org/userspace-api/landlock.html).
- [io_uring](https://github.com/axboe/liburing/blob/master/man/io_uring.7), [cancellation](https://github.com/axboe/liburing/blob/master/man/io_uring_cancelation.7).
- [Plan 9 from Bell Labs](https://9p.io/sys/doc/9.html): private namespaces and substitutable service interfaces, not an obligation to encode all Cell methods as files.
