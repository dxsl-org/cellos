---
title: "Phase 07 - Failover Backpressure and Observability"
status: pending
priority: P1
effort: 4
depends_on: [05, 06]
owner: "runtime-reliability"
---

# Phase 07 - Failover Backpressure and Observability

## Context Links

- Test matrix: `reports/test-matrix.md`
- Rollback/security/failure injections: `reports/rollback-security-failure-injections.md`

## Overview

Priority P1. Make path changes, queue pressure, and remote uncertainty visible and bounded.

## Key Insights

- "Smooth anywhere" means predictable state transitions, not hiding outages.
- `Busy` and `Indeterminate` are user-facing correctness tools.
- Observability must distinguish local, relay, direct, auth, routing, and target-service failures.

## Requirements

- Functional: path state metrics, queue depth counters, dedup hit/miss counters, operationalized Phase 04/05 error taxonomy, failover transitions, deadline/cancel event order, and session-pool pressure handling.
- Non-functional: no periodic unbounded polling; logs must avoid secrets and payload dumps.

## Architecture

Data flow: request lifecycle emits structured events -> counters aggregate per path/peer/export -> test oracle asserts expected state and error surface -> operator can diagnose relay vs direct vs local target failures.

## Related Code Files

- Future owner phase: `cells/services/net-broker/src/main.rs`
- Future owner phase: focused observability module under `cells/services/net-broker/src/`
- Future owner phase: test/oracle harness files only after approval.

## Implementation Steps

1. Operationalize Phase 04/05 taxonomy: `NoService`, `Unreachable`, `Timeout`, `Busy`, `Indeterminate`, `AuthFailed`, `ProtocolError`.
2. Define path states: disabled, relay-registering, relay-ready, direct-probing, direct-ready, degraded.
3. Define counters and log keys.
4. Implement bounded backpressure policy per queue from the already-frozen mapping.
5. Record failover transitions and event order.
6. Monitor no-evict-in-flight invariant from Phase 05.
7. Record deadline/cancel transitions for pre-dispatch timeout, post-dispatch timeout, best-effort cancel, stale response, and duplicate response.

## Todo List

- [ ] Define max log verbosity for normal mode.
- [ ] Define redaction rules.
- [ ] Define queue pressure thresholds.
- [ ] Define path state trace assertions.
- [ ] Define in-flight session eviction prohibition.
- [ ] Define cancel/deadline event ordering assertions.

## Success Criteria

- Each failure injection maps to one typed error.
- Queue pressure never silently drops an accepted request.
- Logs can prove whether relay or LAN path carried a request.
- Pool pressure never evicts in-flight work.
- Stale or late responses are observable and cannot satisfy a newer request.

## Risk Assessment

- Risk: observability itself becomes noisy. Likelihood medium, impact medium. Mitigation: counters by default, detailed traces only in oracle/test mode.
- Risk: wrong error mapping encourages unsafe retry. Likelihood medium, impact high. Mitigation: retry class is carried in envelope and checked at failure site.

## Security Considerations

Never log keys, PSKs, plaintext payloads, or full request bodies. NodeId prefix logs are allowed only as non-secret correlation aids.

## Rollback

Disable direct path and remote exports; local IPC remains available. Observability fields are additive and can remain inert.

## Next Steps

Proceed to failure injection and two-node gates.
