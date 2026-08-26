---
title: "Transport Report"
status: pending
created: 2026-08-19
---

# Transport Report

## Verdict

V1 transport is relay-first for correctness and direct LAN as optimization. Do not start with QUIC, ICE, hole punch, public discovery, or K3.

## Existing Transport Reality

- `ConnectionManager` describes direct TCP then relay fallback, but relay-mediated Noise is still TODO: `cells/services/net-broker/src/connection_manager.rs:83`, `cells/services/net-broker/src/connection_manager.rs:91`.
- `RelayClient` supports raw TCP registration and NodeId-addressed send/receive frame shapes: `cells/services/net-broker/src/relay.rs:74`, `cells/services/net-broker/src/relay.rs:125`, `cells/services/net-broker/src/relay.rs:166`.
- `main.rs` only checks relay liveness today and does not drain relay frames: `cells/services/net-broker/src/main.rs:132`, `cells/services/net-broker/src/main.rs:134`.
- Noise binds NodeIds in prologue, which is sufficient for V1 node-level auth when paired with a stable first-boot key: `cells/services/net-broker/src/transport.rs:139`, `cells/services/net-broker/src/transport.rs:161`.

## V1 Path Classes

- Local: direct kernel IPC, no broker forwarding, no remote failures.
- Relay: broker -> self-hosted relay -> peer broker, E2E Noise payload, relay sees NodeIds and sizes only.
- LAN direct: broker -> TCP connect -> Noise -> C2C frames, used after relay correctness exists.

## Path Selection

- Always establish relay registration first when remote is enabled.
- Use relay for initial correctness oracle and as the stable fallback.
- Attempt LAN direct in parallel only after export lookup and relay path are already functional.
- On direct failure, fail back to relay without changing request id.
- Path changes must not reset deadline, retry class, or dedup state.

## Backpressure

- Per-peer send queue is bounded.
- Per-request payload size is bounded below `IPC_BUF_SIZE` unless a later streaming phase is approved.
- Relay receive loop cannot block local attested ingress; Candidate B uses a dedicated ingress task plus bounded queues.

## Deferred

QUIC, migration-grade mobility, ICE/STUN/TURN public traversal, UDP hole punching, public discovery, relay marketplace, K3/DICE attested enrollment.

## Tests Required

Relay register/auth failure, relay encrypted payload assertion, direct path wins only after handshake and route match, direct break mid-request returns response via relay or `Indeterminate`, relay outage returns `Unreachable`, not `NoService`.
