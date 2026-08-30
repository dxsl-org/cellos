---
title: "Cell-to-Cell Anywhere Core Recovery Plan"
description: "Supersede the blocked 260624 plan with a relay-first, typed endpoint architecture across local, LAN, and remote."
status: in-progress
priority: P1
effort: 33
branch: main
tags: [cell-to-cell-anywhere, recovery, net-broker, relay, distributed-cells]
created: 2026-08-19
---

# Cell-to-Cell Anywhere Core Recovery Plan

Recovery plan: supersedes `.agents/260624-cell-to-cell-anywhere/` without editing it. The required `c2c-broker-oracle-single-guest-local-runtime` CI job records only a single-guest local-runtime QEMU oracle; this plan makes no two-node relay, direct-LAN, remote in-flight restart/failover, hardware, production, or completion claim.

> **Decision record (2026-08-27):** The hardware-independent roadmap selected
> this relay-first recovery plan as the sole Cell-to-Cell transport-ordering
> authority. It supersedes the competing direct-only assumption in
> `260827-1004-hardware-independent-roadmap` Phase 04. Public export and
> distributed leases remain deferred; this decision does not assert runtime,
> QEMU, hardware, or production evidence.

> **Execution boundary (2026-08-29):** The K1 fixture and single-guest baseline
> suite are implemented and CI-gated. The broker now registers with the existing
> KMS ABI, validates exact register/status/acquire snapshots, and can supply
> Clatter with an opaque KMS static-DH handle/epoch/public key without importing
> the private scalar. Plaintext VFS `machine-id` is retired as a C2C identity
> root. KMS absence, non-ready provider state, or any mixed snapshot selects an
> ephemeral local-only identity and keeps remote disabled. Candidate B local
> ingress is complete at the single-guest ceiling: focused host tests pass
> 63/63; the restart-enabled RV64 oracle passes 1/1, 1,000 measured calibration
> successes, role gating, all 1/2/4/8/16-client sweeps, a 10,000/10,000 soak
> with zero silent drops and positive network progress, bounded overflow, zero
> kernel heartbeat/watchdog termination markers, and supervised broker restart
> with clean role drain, stale old-TID failure, fresh state, and successful retry.
>
> Operator recovery is fixed to a live-supervisor exact nonzero-revision CAS;
> qualified provider execution and physical recovery evidence remain open.
> Phase 04 is complete at the disabled local-only ceiling: canonical bounded
> envelope/decode, fixed no-evict-in-flight cache, boot-local server epochs,
> explicit typed endpoints, monotonic deadlines, and epoch-before-dedup receive
> ordering pass 92/92 broker host tests plus 5/5 endpoint integration tests and
> RV64 builds.
>
> Phase 05 local-only contract work has started without relay enablement. The
> four-session Noise pool now fails closed on exhaustion, preserves all occupied
> sessions, and reports `WouldBlock` before opening another TCP path. Noise
> prologues use protocol-role ordering, so both peers bind the same
> `initiator || responder` identity transcript. The optional global relay
> endpoint is now a strict, allocation-free `cluster.cfg` contract with private
> representation and read-only accessors, so its validated invariant survives
> storage without dialing.
> Server admission treats the validated mTLS certificate-derived NodeId as the
> sole route authority, rejects duplicate live identities without displacement,
> and uses exact-generation release. Relay reconnect delay uses equal jitter,
> grows exponentially to a 30-second ceiling, and resets only after an
> authenticated session is established. Exact byte-layout, paired transcript,
> endpoint parser, and reconnect regressions pass within 105/105 broker tests;
> 37/37 relay-server tests cover the bounded pre-TLS gate, authenticated
> admission, missing/wrong `clientAuth` and non-P-256 rejection, wire behavior,
> and the definite destination-missing versus accepted-then-uncertain
> write-failure split. The RV64 broker release build also passes.
> ADR-0008 now fixes relay target binding: the Protected Relay Authority owns
> server verification, client authentication, TLS secrets, and record crypto;
> service-net carries only bounded bytes, and the old transcript-hash signer
> denies in production. The isolated oracle topology, deterministic predicates,
> and authenticated evidence contract are defined, but no client or oracle run
> is claimed.
>
> The two-real-broker relay path remains blocked by protected persistence,
> authenticated time, reviewed pending-key binding, DEV_REFERENCE Phase 8 GO,
> implementation of the approved authority-owned TLS endpoint, and post-Build
> AC-012 hostile-path evidence. These gates remain tracked in the KMS/Silo
> Phase 4 plan. No two-node, relay, direct-LAN, hardware, or production claim is
> made.

## Verdict

Candidate B is default: explicit Local/Remote typed endpoints; local direct kernel IPC; userspace `net-broker` for LAN/remote; protected KMS-owned stable X25519 node identity with opaque static DH; dedicated blocking `sys_recv_attested` ingress task feeding bounded in-cell queues; relay-first correctness; direct LAN as optimization.

Candidate A, attested `TryRecv` parity, is only a contingency after reproducible failure against frozen latency/watchdog/queue targets, root cause specifically blocks ingress, no userspace repair exists, and Law 1 receives two confirmations.

## Phases

| # | Phase | Effort | Depends on | Law 1 |
|---|---|---:|---|---|
| 01 | [Recovery baseline and contract freeze](./phase-01-recovery-baseline-and-contract-freeze.md) | 3 | none | no |
| 02 | [Stable node identity and exported endpoint registry](./phase-02-stable-node-identity-and-exported-endpoint-registry.md) | 4 | 01 | no |
| 03 | [Broker ingress task and bounded local queues](./phase-03-broker-ingress-task-and-bounded-local-queues.md) | 4 | 01; consumes Phase 02 fail-closed policy | no |
| 04 | [C2C envelope, request semantics, and dedup](./phase-04-c2c-envelope-request-semantics-and-dedup.md) | 5 | 02,03 | no |
| 05 | [Relay-first remote correctness oracle](./phase-05-relay-first-remote-correctness-oracle.md) | 5 | 04 | no |
| 06 | [Direct LAN Noise optimization](./phase-06-direct-lan-noise-optimization.md) | 3 | 05 | no |
| 07 | [Failover, backpressure, and observability](./phase-07-failover-backpressure-and-observability.md) | 4 | 05,06 | no |
| 08 | [Failure injection and isolated two-node gates](./phase-08-failure-injection-and-two-node-gates.md) | 3 | 07 | no |
| 09 | [Rollout, docs handoff, and Candidate A contingency](./phase-09-rollout-docs-and-contingency.md) | 2 | 08 | possible |

## Key Artifacts

- Research: [old artifact audit](./research/research-audit.md), [semantics](./research/semantics-report.md), [transport](./research/transport-report.md)
- Reports: [scout](./reports/scout-report.md), [assumptions](./reports/assumptions.md), [dependency graph](./reports/dependency-graph.md), [test matrix](./reports/test-matrix.md)
- Gates: [rollback/security/failure injections](./reports/rollback-security-failure-injections.md), [success gates](./reports/success-gates.md), [red team](./reports/red-team-review.md), [validation](./reports/validation.md)
- Visual: [architecture Mermaid](./visuals/cell-to-cell-anywhere-architecture.mmd), [SVG](./visuals/cell-to-cell-anywhere-architecture.svg), [PNG](./visuals/cell-to-cell-anywhere-architecture.png)

## Non-negotiables

- V1 includes explicit exports, node-level auth, C2C envelope, `request_id`, boot/server epoch, relative deadline, retry class, bounded dedup, `Busy` and `Indeterminate` semantics, direct TCP+Noise, authenticated self-hosted relay with end-to-end Noise, isolated LAN and relay oracles, failover, and observability.
- Before code: freeze local IPC p99 baseline with <=5% regression budget, zero watchdog expirations, queue/cache memory budgets, 10k accepted unary-call soak, and measured broker concurrency/saturation target.
- Defer QUIC, ICE, hole punch, public discovery, K3, remote watch ABI, remote VFS, and distributed leases.
- Red Team Review and validation are `PASS_WITH_RISK`; production/complete status remains blocked until retained relay and LAN oracle evidence exists.
