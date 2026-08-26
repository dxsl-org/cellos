---
title: "Cell-to-Cell Anywhere Core Recovery Plan"
description: "Supersede the blocked 260624 plan with a relay-first, typed endpoint architecture across local, LAN, and remote."
status: pending
priority: P1
effort: 33
branch: main
tags: [cell-to-cell-anywhere, recovery, net-broker, relay, distributed-cells]
created: 2026-08-19
---

# Cell-to-Cell Anywhere Core Recovery Plan

Recovery plan: supersedes `.agents/260624-cell-to-cell-anywhere/` without editing it. This plan does not claim tests, CI, QEMU, relay, LAN, or hardware evidence.

## Verdict

Candidate B is default: explicit Local/Remote typed endpoints; local direct kernel IPC; userspace `net-broker` for LAN/remote; stable first-boot X25519 node key; dedicated blocking `sys_recv_attested` ingress task feeding bounded in-cell queues; relay-first correctness; direct LAN as optimization.

Candidate A, attested `TryRecv` parity, is only a contingency after reproducible failure against frozen latency/watchdog/queue targets, root cause specifically blocks ingress, no userspace repair exists, and Law 1 receives two confirmations.

## Phases

| # | Phase | Effort | Depends on | Law 1 |
|---|---|---:|---|---|
| 01 | [Recovery baseline and contract freeze](./phase-01-recovery-baseline-and-contract-freeze.md) | 3 | none | no |
| 02 | [Stable node identity and exported endpoint registry](./phase-02-stable-node-identity-and-exported-endpoint-registry.md) | 4 | 01 | no |
| 03 | [Broker ingress task and bounded local queues](./phase-03-broker-ingress-task-and-bounded-local-queues.md) | 4 | 01,02 | no |
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
