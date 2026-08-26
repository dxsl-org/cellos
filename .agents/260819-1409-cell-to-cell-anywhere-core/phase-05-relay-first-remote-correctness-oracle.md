---
title: "Phase 05 - Relay-First Remote Correctness Oracle"
status: pending
priority: P1
effort: 5
depends_on: [04]
owner: "relay-runtime"
---

# Phase 05 - Relay-First Remote Correctness Oracle

## Context Links

- Transport: `research/transport-report.md`
- Success gates: `reports/success-gates.md`

## Overview

Priority P1. Make remote work through an authenticated self-hosted relay first, before optimizing direct LAN.

## Key Insights

- Relay framing exists but inbound relay frames are not wired.
- Relay-first gives a stable correctness path for NAT-hostile and remote environments.
- Payload must remain end-to-end Noise encrypted.

## Requirements

- Functional: relay register, relay registration proof, duplicate NodeId rejection, peer send, peer receive, relay-mediated Noise session, C2C request/response, relay-only two-node oracle.
- Non-functional: self-hosted relay, no public relay dependency, bounded frame size, heartbeat-safe receive cadence.

## Entry Gate

- Phase 04 has frozen typed client API ownership, error taxonomy, retry mapping, and dedup states.
- No in-flight request may be evicted from a session/path pool; pressure returns `Busy` or path-specific `Indeterminate`.
- Relay oracle must include dedup expiry and dedup exhaustion cases before any remote-complete claim.

## Architecture

Data flow: node A broker -> relay TCP register -> E2E Noise to node B over relay packets -> C2C request -> node B local export -> response -> relay -> node A dedup/response.

## Related Code Files

- Future owner phase: `cells/services/net-broker/src/relay.rs`
- Future owner phase: `cells/services/net-broker/src/connection_manager.rs`
- Future owner phase: `cells/services/net-broker/src/main.rs`
- Future owner phase: relay test harness outside product path, if approved.

## Implementation Steps

1. Specify relay server authentication, registration proof, and duplicate NodeId rejection.
2. Specify relay-mediated Noise handshake.
3. Wire relay receive into broker dispatcher.
4. Add relay-only path selection and health state.
5. Define isolated two-node relay oracle with no hardware evidence claim.
6. Define sender-visible errors for destination-missing, relay write failure, and relay disconnect.
7. Verify retry mapping over relay for `Busy`, `Unreachable`, `Timeout`, and `Indeterminate`.

## Todo List

- [ ] Define relay config format.
- [ ] Define relay auth secret or node allowlist.
- [ ] Define proof-of-possession or Noise-bound registration.
- [ ] Define duplicate NodeId/hijack rejection oracle.
- [ ] Define relay reconnect backoff.
- [ ] Define oracle topology and logs retained.
- [ ] Define no-evict-in-flight relay pool behavior.
- [ ] Define dedup expiry/exhaustion relay oracle.

## Success Criteria

- Two isolated nodes complete one exported service request via relay.
- Relay sees NodeIds and ciphertext only.
- Relay disconnect maps to `Unreachable` or `Indeterminate`, not `NoService`.
- Missing destination and relay forward failure never silently drop an accepted request.
- Duplicate NodeId registration is rejected or quarantined before packet forwarding.
- In-flight relay requests are never evicted; pressure is visible to caller.

## Risk Assessment

- Risk: relay becomes trusted data plane. Likelihood low, impact high. Mitigation: E2E Noise; relay cannot decrypt payload.
- Risk: relay receive blocks broker. Likelihood medium, impact high. Mitigation: bounded queues and heartbeat discipline.

## Security Considerations

Relay is authenticated and self-hosted. Relay compromise cannot forge Noise-authenticated node identity. Registration must prove ownership of the advertised NodeId before the relay table is mutated.

## Rollback

Disable relay remote mode and keep local endpoint path plus plan artifacts. No kernel state requires rollback.

## Next Steps

Proceed to direct LAN Noise optimization after relay oracle passes.
