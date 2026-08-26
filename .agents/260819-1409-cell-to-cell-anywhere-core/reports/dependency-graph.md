---
title: "Dependency Graph"
status: pending
created: 2026-08-19
---

# Dependency Graph

## Phase Graph

```text
01 baseline
  -> 02 identity/export
02 -> 03 ingress/queues
02 + 03 -> 04 envelope/dedup
04 -> 05 relay oracle
05 -> 06 LAN direct
05 + 06 -> 07 failover/observability
07 -> 08 failure injections/two-node gates
08 -> 09 rollout/contingency
```

## File Ownership By Phase

- 01: plan docs only.
- 02: future ownership of `identity.rs`, export registry module/config.
- 03: future ownership of broker runtime/queue modules and `main.rs` ingress wiring.
- 04: future ownership of C2C protocol/dedup modules and routing integration.
- 05: future ownership of `relay.rs`, relay-mediated session wiring, relay oracle harness.
- 06: future ownership of `connection_manager.rs` and direct path health.
- 07: future ownership of observability/backpressure module and status taxonomy.
- 08: future ownership of integration/oracle harness.
- 09: future ownership of docs handoff and Candidate A decision package.

## Parallelism Rule

No two implementation phases should modify the same product file in parallel. Phases 02-07 touch `net-broker`; they are sequential unless split into non-overlapping modules with a single integrator.

## Blocking Dependencies

- No C2C frame before stable node id and ingress queue contract.
- No relay oracle before envelope semantics and dedup.
- No LAN optimization before relay oracle.
- No docs "complete" status before Phase 08 gates.
- No Candidate A before Phase 09 conditions and Law-1 confirmations.
