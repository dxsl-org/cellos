---
title: "Phase 06 - Direct LAN Noise Optimization"
status: pending
priority: P1
effort: 3
depends_on: [05]
owner: "lan-path"
---

# Phase 06 - Direct LAN Noise Optimization

## Context Links

- Transport: `research/transport-report.md`
- Dependency graph: `reports/dependency-graph.md`

## Overview

Priority P1. Add direct TCP+Noise LAN path after relay correctness exists. LAN is an optimization, not the first source of truth.

## Key Insights

- `ConnectionManager` already intends direct TCP before relay, but current plan reverses product validation order: relay correctness first, LAN optimization second.
- Direct path must reuse the same C2C envelope, request id, deadline, and dedup state.

## Requirements

- Functional: direct address config, TCP connect, Noise handshake, route health, fallback to relay.
- Non-functional: no NAT traversal in this phase; no QUIC; bounded retries; observable path choice.

## Architecture

Data flow: remote endpoint request -> relay path already available -> direct TCP candidate connects -> Noise validates NodeIds -> path health becomes direct-preferred -> same C2C frame flows over direct path.

## Related Code Files

- Future owner phase: `cells/services/net-broker/src/connection_manager.rs`
- Future owner phase: `cells/services/net-broker/src/transport.rs`
- Future owner phase: `cells/services/net-broker/src/identity.rs`

## Implementation Steps

1. Define direct candidate source: static config first.
2. Define direct connect timeout and retry backoff.
3. Define direct path health transitions.
4. Define fallback to relay without changing request id.
5. Add LAN-only two-node oracle after relay oracle passes.

## Todo List

- [ ] Choose direct timeout.
- [ ] Define direct health probe.
- [ ] Define preferred path hysteresis.
- [ ] Define logs proving direct vs relay path.

## Success Criteria

- Direct LAN path can carry the same exported request as relay.
- Breaking direct path falls back to relay within deadline when possible.
- Within the authenticated retention window, direct failure cannot duplicate local dispatch; outside it, non-idempotent outcome is `Indeterminate`.

## Risk Assessment

- Risk: direct path racing adds state bugs. Likelihood medium, impact high. Mitigation: no racing until relay path is green; use one active request state machine.
- Risk: LAN-only success masks remote failure. Likelihood medium, impact high. Mitigation: relay oracle remains mandatory.

## Security Considerations

Direct TCP is never plaintext C2C. It must complete Noise with expected peer NodeId before carrying frames.

## Rollback

Turn off direct path selection and force relay path. Dedup and endpoint semantics remain unchanged.

## Next Steps

Proceed to failover, backpressure, and observability.
