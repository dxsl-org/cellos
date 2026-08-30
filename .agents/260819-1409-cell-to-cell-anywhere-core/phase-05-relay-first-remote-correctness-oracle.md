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
- Protected TLS ownership:
  `../../docs/decisions/0008-protected-relay-tls-endpoint-ownership.md`
- Correlated relay framing:
  `../../docs/decisions/0009-correlate-relay-packet-failures.md`
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
- ADR-0009 retires ambiguous uncorrelated sends before any Cellos relay client exists.

## Requirements

- Functional: certificate-derived connection admission, duplicate NodeId
  rejection, peer send, peer receive, relay-mediated Noise session, C2C
  request/response, and a relay-only two-node oracle.
- Non-functional: self-hosted relay, no public relay dependency, bounded
  pre-TLS connections/sessions/frames, and heartbeat-safe receive cadence.
- Wire contract: `FT_SEND_PACKET_CORRELATED (0x0d)` carries a nonzero per-TLS-session `u64be` correlation before the destination NodeId; `FT_PACKET_ERROR (0x0a)` returns that correlation plus one definite/uncertain code. Legacy `0x08` and malformed server input are connection-fatal. Server codec work may proceed locally; the authority-owned client codec and correlation lifecycle wait for post-entry-GO Phase 4 Build.
- Correlation lifecycle: active current-generation errors resolve once; retired lower-than-next values are ignored and bounded-counted; zero/future/unaccepted or stale-generation input never selects a request. Only explicit authority rejection with typed-request ownership returned remains `NotSubmitted`; acceptance or an ambiguous send outcome is `Submitted`, so disconnect maps unresolved work to `Indeterminate`.

## Entry Gate

- Phase 04 has frozen typed client API ownership, error taxonomy, retry mapping, and dedup states.
- No in-flight request may be evicted from a session/path pool; pressure returns `Busy` or path-specific `Indeterminate`.
- Relay oracle must include dedup expiry and dedup exhaustion cases before any remote-complete claim.

## Architecture

Data flow: node A broker -> bounded TLS 1.3 connection -> certificate-derived
NodeId admission -> E2E Noise to node B over relay packets -> C2C request ->
node B local export -> response -> relay -> node A dedup/response.

ADR-0008 binds both relay server validation and device client authentication to
one protected TLS state machine. The Protected Relay Authority owns server
chain/hostname/time checks, transcript and Finished verification, the active
client profile, CertificateVerify, traffic secrets, and TLS records.
`service-net` is only the fixed-endpoint bounded byte carrier. Net-broker's
correct typed path sends and receives Noise-record buffers; the authority treats
them as opaque and cannot prove ciphertext provenance after application-processor
compromise. Public KMS opcodes 9–14 remain unchanged, and the legacy opaque
transcript-hash signer denies in production. Generic TLS,
caller-selected identity, shared-secret, raw-key, and insecure fallback remain
forbidden.

ADR-0009 cleanly retires `FT_SEND_PACKET (0x08)`. The protected authority emits
`0x0d || correlation:u64be || destination:NodeId[32] || Noise ciphertext` from a
typed broker request and parses request-scoped `FT_PACKET_ERROR (0x0a)` into a
typed generation-bound event. Uncorrelated `FT_ERROR (0x7f)` remains only for
fatal protocol errors followed by close. Correlation is nonzero, strictly
increasing, never reused within one authenticated TLS session, and locally keyed
with relay session generation. Net-broker never constructs/parses raw outer
frames; `FT_RECV_PACKET` and the opaque Noise envelope stay unchanged.

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
2. Replace legacy server send/error framing with ADR-0009 correlated packet failures.
3. During post-entry-GO Phase 4 Build, add the authority-owned outer codec and typed broker correlation lifecycle.
4. Specify relay-mediated Noise handshake.
5. Wire typed relay receive events into broker dispatcher with bounded session-generation correlation state.
6. Add relay-only path selection and health state.
7. Define isolated two-node relay oracle with no hardware evidence claim.
8. Verify exact sender-visible mapping for destination absence, uncertain forwarding, relay disconnect, `Busy`, `Timeout`, and `Indeterminate`.

## Todo List

- [x] Define the optional global relay endpoint as strict `relay_ip`,
  `relay_port`, and lowercase DNS `relay_hostname` fields in `cluster.cfg`.
- [x] Preserve the validated endpoint invariant with private representation and
  read-only `ip()`, `port()`, and `hostname()` accessors.
- [x] Define protected mTLS target binding through ADR-0008 authority-owned TLS;
  fixed trust/profile inputs alone are insufficient.
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
- [x] Approve ADR-0009 clean correlated framing; reject legacy dual-stack and single-outstanding-request fallback.
- [x] Implement server-side exact `0x0d` send and `0x0a` packet-error layouts; reject retired `0x08` and malformed framing.
- [ ] After Phase 4 entry GO, implement the authority-owned client codec, typed request ownership, and broker active/retired correlation state.

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

ADR-0009 server framing is complete at the host-service ceiling. Do not add a
raw net-broker codec: the authority-owned client codec and broker correlation
integration wait for post-entry-GO Phase 4 Build. ADR-0008 resolves TLS endpoint
ownership, but protected persistence, authenticated time, pending-key binding,
and the DEV_REFERENCE Phase 8 GO over AC-001..AC-011 remain unsatisfied. After
that GO, Phase 4 must implement ADR-0008/0009 and pass AC-012 before this relay
route can open. No sender-side mapping, relay-oracle execution, or two-node
result is available.

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
  split, certificate-policy negatives, and correlated wire regressions pass
  40/40 relay-server tests. The server accepts only nonzero-correlated `0x0d`
  sends, preserves opaque payloads in unchanged `0x09` receive frames, echoes
  exact correlations in `0x0a` definite/uncertain errors, rejects retired `0x08`,
  zero/malformed/unknown input with fatal `0x7f` and close, and keeps two
  interleaved failures distinct. Missing/wrong `clientAuth` EKU and non-P-256
  keys fail before route admission. Duplicate same-certificate connections
  cannot interrupt or reroute the established session, and stale generation
  cleanup cannot remove a later explicit admission. Compile 4/4, focused tester,
  production-readiness, and security reviews pass.
- ADR-0008 now fixes target binding: the protected authority owns the complete
  relay TLS endpoint, service-net carries only bounded bytes, and the old public
  transcript-hash signer denies in production. This is an approved architecture,
  not an implemented client. The isolated topology, deterministic failure
  predicates, privacy rules, and externally attested/replay-protected evidence
  bundle are defined; no oracle run is claimed.
- No Cellos relay client, remote dispatch, receive loop, or two-node traffic is
  enabled or claimed.
