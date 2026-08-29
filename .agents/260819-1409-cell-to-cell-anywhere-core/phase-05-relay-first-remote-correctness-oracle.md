---
title: "Phase 05 - Relay-First Remote Correctness Oracle"
status: in_progress
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

- Functional: certificate-derived connection admission, duplicate NodeId
  rejection, peer send, peer receive, relay-mediated Noise session, C2C
  request/response, and a relay-only two-node oracle.
- Non-functional: self-hosted relay, no public relay dependency, bounded
  pre-TLS connections/sessions/frames, and heartbeat-safe receive cadence.

## Entry Gate

- Phase 04 has frozen typed client API ownership, error taxonomy, retry mapping, and dedup states.
- No in-flight request may be evicted from a session/path pool; pressure returns `Busy` or path-specific `Indeterminate`.
- Relay oracle must include dedup expiry and dedup exhaustion cases before any remote-complete claim.

## Architecture

Data flow: node A broker -> bounded TLS 1.3 connection -> certificate-derived
NodeId admission -> E2E Noise to node B over relay packets -> C2C request ->
node B local export -> response -> relay -> node A dedup/response.

## Related Code Files

- `cells/services/net-broker/src/session_pool.rs`
- `cells/services/net-broker/src/transport.rs`
- `cells/services/net-broker/src/transport/connection_pool.rs`
- `cells/services/net-broker/src/transport/noise_session.rs`
- `cells/services/net-broker/src/noise_identity.rs`
- `cells/services/net-broker/src/noise_identity/tests.rs`
- `cells/services/net-broker/src/transport/tcp_framing.rs`
- `cells/services/net-broker/src/connection_manager.rs`
- `cells/services/net-broker/src/relay_config.rs`
- `cells/services/net-broker/src/relay_config/tests.rs`
- `cells/services/net-broker/src/peer_config/ascii.rs`
- `cells/services/net-broker/src/identity.rs`
- `tools/relay-server/relay_admission.py`
- `tools/relay-server/relay_admission_test.py`
- `tools/relay-server/relay.py`
- `tools/relay-server/relay_test.py`
- `tools/relay-server/relay_cancellation_test.py`
- `tools/relay-server/relay_bootstrap.py`
- `tools/relay-server/relay_bootstrap_test.py`
- `tools/relay-server/_relay_certificate_support.py`
- `tools/relay-server/_relay_test_support.py`
- Future relay runtime owner: `cells/services/net-broker/src/main.rs`
- Future relay test harness outside product path, if approved.

## Implementation Steps

1. Specify certificate-derived relay admission and duplicate NodeId rejection.
2. Specify relay-mediated Noise handshake.
3. Wire relay receive into broker dispatcher.
4. Add relay-only path selection and health state.
5. Define isolated two-node relay oracle with no hardware evidence claim.
6. Define sender-visible errors for destination-missing, relay write failure, and relay disconnect.
7. Verify retry mapping over relay for `Busy`, `Unreachable`, `Timeout`, and `Indeterminate`.

## Todo List

- [x] Define the optional global relay endpoint as strict `relay_ip`,
  `relay_port`, and lowercase DNS `relay_hostname` fields in `cluster.cfg`.
- [ ] Define mTLS trust/profile inputs and signer authorization; no shared-secret
  or raw-key fallback.
- [x] Use the validated mTLS certificate-derived NodeId as registration proof;
  no advertised identity, proof blob, shared secret, or raw-key fallback can
  become route authority.
- [x] Reject unauthenticated, identity-mismatched, duplicate-live, capacity, and
  stale-disconnect admission transitions without displacing a live route.
- [ ] Define relay reconnect backoff.
- [ ] Define oracle topology and logs retained.
- [x] Define no-evict session-pool admission: full capacity returns explicit
  pressure without opening another TCP path or displacing an existing session.
- [x] Canonicalize the Noise prologue as
  `cluster_id || initiator_node_id || responder_node_id` and prove a paired
  initiator/responder KKpsk0 transcript completes.
- [x] Pin the 72-byte prologue layout for both local roles: little-endian
  cluster ID, then initiator NodeId, then responder NodeId.
- [ ] Define dedup expiry/exhaustion relay oracle.

## Success Criteria

- Two isolated nodes complete one exported service request via relay.
- Relay sees NodeIds and ciphertext only.
- Relay disconnect maps to `Unreachable` or `Indeterminate`, not `NoService`.
- Missing destination and relay forward failure never silently drop an accepted request.
- A duplicate live certificate-derived NodeId is rejected before packet
  forwarding and cannot displace the established route.
- In-flight relay requests are never evicted; pressure is visible to caller.

## Risk Assessment

- Risk: relay becomes trusted data plane. Likelihood low, impact high. Mitigation: E2E Noise; relay cannot decrypt payload.
- Risk: relay receive blocks broker. Likelihood medium, impact high. Mitigation: bounded queues and heartbeat discipline.

## Security Considerations

Relay is authenticated and self-hosted. Relay compromise cannot forge
Noise-authenticated node identity. TLS and certificate-policy validation derive
the sole admitted NodeId before route-table mutation; there is no advertised
identity claim or registration proof frame.

## Rollback

Disable relay remote mode and keep local endpoint behavior. The bounded session
pool requires no rollback because it only removes unsafe LRU displacement. No
kernel state requires rollback.

## Next Steps

Continue local-only relay contract work with reconnect backoff and
sender-visible delivery-error semantics. Protected client signer qualification
still blocks Cellos relay wiring and any two-node oracle.

## Local Contract Evidence

- `BoundedSessionPool<T, 4>` admits only into empty slots. A full pool returns
  ownership unchanged; explicit removal opens one slot without touching
  survivors.
- Binary `ConnectionPool` maps capacity pressure to `ViError::WouldBlock`.
  `ConnectionManager` checks capacity before `TcpConnect` and propagates that
  pressure instead of falling through to `NotSupported`.
- The optional global relay endpoint parser fails closed on partial, duplicate,
  unknown, malformed IP/port, or non-canonical hostname fields and performs no
  I/O. `BrokerIdentity` stores only a validated endpoint without dialing it.
- Focused broker tests pass 101/101 and the RV64 release build passes.
- The pure relay admission table, pre-TLS connection gate, and mTLS wire
  regressions pass within 33/33 relay-server tests. Duplicate same-certificate
  connections close without interrupting or rerouting the established session;
  stale generation cleanup cannot remove a later explicit admission. Tester and
  production-readiness reviewer rechecks pass.
- No Cellos relay client, remote dispatch, receive loop, or two-node traffic is
  enabled or claimed.
