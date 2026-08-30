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
- Relay identity decision: `../../docs/decisions/0005-mutual-tls-relay-identity.md`
- Protected signer gate:
  `../260825-1726-kms-silo-production-root/phase-04-service-net-mutual-tls-integration.md`
- Approved protected profile contract:
  `../260825-1726-kms-silo-production-root/spec.md`

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

The server trust inputs and active client-certificate profile are fixed, not
caller-selected. Signer authorization is not yet complete: current frozen
requests let untrusted service-net submit an opaque transcript hash after doing
its own relay CA/hostname validation, so the protected authority cannot prove
that it is authenticating the configured relay rather than an attacker server.
Remote wiring stays blocked until an approved architecture binds protected
signing to the exact server chain, hostname/endpoint, handshake, live broker
generation, and active client tuple without trusting service-net assertions.
Generic client identity and shared-secret/raw-key fallback remain forbidden.

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
- `cells/services/net-broker/src/relay_reconnect.rs`
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
- [x] Preserve the validated endpoint invariant with private representation and
  read-only `ip()`, `port()`, and `hostname()` accessors.
- [ ] Define protected mTLS signer authorization bound to the exact authenticated
  relay server identity; fixed trust/profile inputs alone are insufficient.
- [x] Exercise missing/wrong `clientAuth` EKU and non-P-256 certificate
  rejection before route admission.
- [x] Use the validated mTLS certificate-derived NodeId as registration proof;
  no advertised identity, proof blob, shared secret, or raw-key fallback can
  become route authority.
- [x] Reject unauthenticated, identity-mismatched, duplicate-live, capacity, and
  stale-disconnect admission transitions without displacing a live route.
- [x] Define relay reconnect backoff with equal jitter, exponential growth, a
  30-second ceiling, and reset only after authenticated session establishment.
- [x] Separate definite pre-write destination absence from accepted-then-uncertain
  destination write/drain failure with bounded relay error codes.
- [x] Define isolated oracle topology and privacy-safe retained evidence.
- [x] Define no-evict session-pool admission: full capacity returns explicit
  pressure without opening another TCP path or displacing an existing session.
- [x] Canonicalize the Noise prologue as
  `cluster_id || initiator_node_id || responder_node_id` and prove a paired
  initiator/responder KKpsk0 transcript completes.
- [x] Pin the 72-byte prologue layout for both local roles: little-endian
  cluster ID, then initiator NodeId, then responder NodeId.
- [x] Define deterministic dedup expiry/exhaustion relay-oracle predicates.

## Oracle Topology and Retained Evidence

- One self-hosted relay and two Cellos node instances run in separate network
  namespaces. Namespace ACLs permit each node to reach only the relay and deny
  node-to-node traffic. Distinct CA-issued identities, state directories, and
  NodeIds are mandatory; both nodes share only the intended cluster and K1.
  Retained ACL/route snapshots and successful negative direct-connect probes
  prove that absent direct candidates are enforced rather than assumed.
- The broker emits a selected-path event for the exact request ID. The run
  requires `relay` for that request, one exported response, and unchanged request
  ID through disconnect/reconnect. Any direct-path event or missing correlation
  fails the run.
- Deterministic lanes require: duplicate-before-completion returns `Busy`
  without redispatch; retained completion replays the same result; expired
  non-idempotent completion returns `Indeterminate` without redispatch; a cache
  full of in-flight entries returns `Busy` without eviction; expired completed
  state releases capacity; pre-dispatch expiry returns `Timeout`; post-dispatch
  expiry or disconnect after possible delivery returns `Indeterminate`; definite
  pre-write destination absence returns `Unreachable`.
- The authoritative bundle uses `cellos.authenticated-evidence/v1` in the pinned
  GitHub-hosted workflow. A canonical manifest hashes every retained member
  except itself; GitHub attests that manifest digest. The attested digest,
  workflow identity, revision, run-id/attempt, lane identities, and a fresh
  operator-issued oracle nonce are anchored outside the bundle and consumed
  once through the operator-owned replay store.
- Separate relay, node-A, and node-B logs, ACL/route snapshots, negative probes,
  machine-readable predicates, and result summary are retained. Every retained
  member is scanned. Commands are canonical argument arrays containing secret
  references, never expanded secret values; nodes use collision-checked
  per-run opaque aliases rather than stable NodeId prefixes.
- Payloads, request bodies, K1, private/signature material, certificate bodies,
  full NodeIds, unrestricted environments, missing members/hashes, forbidden
  content, failed isolation, or unclassified outcomes fail admission. Host/QEMU
  evidence proves only the isolated software path, never hardware or production.

## Success Criteria

- Two isolated nodes complete one exported service request via relay.
- Relay sees NodeIds and ciphertext only.
- Missing destination is a definite pre-write rejection; destination write or
  drain failure is explicitly uncertain and never silently treated as
  non-delivery. Relay disconnect maps to `Unreachable` or `Indeterminate`, not
  `NoService`.
- A duplicate live certificate-derived NodeId is rejected before packet
  forwarding and cannot displace the established route.
- In-flight relay requests are never evicted; pressure is visible to caller.
- Dedup, deadline, disconnect, and reconnect lanes meet every deterministic
  predicate above with no duplicate local dispatch or in-flight eviction.

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

Continue local-only relay contract work only where it does not cross the blocked
client signer boundary. Resolve protected relay-server identity binding before
client implementation; current opaque transcript-hash authorization is
insufficient. Correlated client framing still requires a separately approved
wire revision. These blockers prevent sender-side mapping, relay-oracle
execution, and any two-node result.

## Local Contract Evidence

- `BoundedSessionPool<T, 4>` admits only into empty slots. A full pool returns
  ownership unchanged; explicit removal opens one slot without touching
  survivors.
- Binary `ConnectionPool` maps capacity pressure to `ViError::WouldBlock`.
  `ConnectionManager` checks capacity before `TcpConnect` and propagates that
  pressure instead of falling through to `NotSupported`.
- The optional global relay endpoint parser fails closed on partial, duplicate,
  unknown, malformed IP/port, or non-canonical hostname fields and performs no
  I/O. `RelayEndpoint` fields are private, so callers cannot construct zero
  ports, invalid hostnames, inconsistent lengths, or nonzero padding; read-only
  accessors expose only parser-validated values. `BrokerIdentity` stores the
  endpoint without dialing it. The relay reconnect contract uses allocation-free
  equal jitter over exponentially growing windows, caps delays at 30 seconds,
  and resets only after authenticated session establishment. Focused broker
  tests pass 105/105 and the RV64 release build passes.
- The pure relay admission table, pre-TLS connection gate, delivery-outcome
  split, certificate-policy negatives, and mTLS wire regressions pass within
  37/37 relay-server tests. Missing/wrong `clientAuth` EKU and non-P-256 keys
  fail before route admission. A missing destination returns definite
  `ERR_DESTINATION_UNAVAILABLE`; a destination write/drain failure after bytes
  may be queued returns `ERR_DELIVERY_UNCERTAIN`. Duplicate same-certificate
  connections cannot interrupt or reroute the established session, and stale
  generation cleanup cannot remove a later explicit admission. Tester and
  production-readiness reviewer rechecks pass.
- Server trust inputs and the active client profile are fixed, but protected
  signer authorization is explicitly incomplete because the current authority
  cannot verify the relay server behind an opaque caller-supplied transcript
  hash. The isolated topology, deterministic failure predicates, privacy rules,
  and externally attested/replay-protected evidence bundle are defined; no
  oracle run is claimed.
- No Cellos relay client, remote dispatch, receive loop, or two-node traffic is
  enabled or claimed.
