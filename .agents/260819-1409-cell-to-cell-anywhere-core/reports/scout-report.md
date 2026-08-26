---
title: "Scout Report"
status: pending
created: 2026-08-19
---

# Scout Report

## Verdict

Current repo has a credible distributed-cell foundation, but Cell-to-Cell Anywhere is not an end-to-end runtime yet. This report is planning evidence only; it is not a test result.

## Code Reality

- `net-broker` declares itself as the cross-machine trust anchor and lists P04-P09 responsibilities: `cells/services/net-broker/src/main.rs:5`, `cells/services/net-broker/src/main.rs:7`.
- The broker generates a per-run X25519 keypair and derives `BrokerIdentity` from it: `cells/services/net-broker/src/main.rs:104`, `cells/services/net-broker/src/main.rs:106`.
- The broker dispatch loop checks relay liveness only and has TODOs for relay, beacon, lease, and dispatch: `cells/services/net-broker/src/main.rs:132`, `cells/services/net-broker/src/main.rs:136`, `cells/services/net-broker/src/main.rs:153`.
- `ClusterRef` returns broker tid for remote lookup but later forwarding is not proven in current code: `libs/ostd/src/cluster.rs:54`, `libs/ostd/src/cluster.rs:72`.
- `RoutingTable` and `RemoteServiceProxy` exist but `routing.rs` says they are not wired from `main.rs`: `cells/services/net-broker/src/routing.rs:1`, `cells/services/net-broker/src/routing.rs:4`.
- `RelayClient` defines raw TCP relay frames and blocking receive but states send/register/receive are not wired into dispatch: `cells/services/net-broker/src/relay.rs:8`, `cells/services/net-broker/src/relay.rs:30`.
- `sys_recv_attested` exists and writes caller identity into the receive buffer tail: `libs/ostd/src/syscall.rs:963`, `libs/api/src/abi/caller_identity.rs:11`.
- `sys_try_recv` exists but passes no attestation flag: `libs/ostd/src/syscall.rs:983`.

## Current Call/Lifetime Check

- Broker runtime scope is process/cell-wide, not per request; `cell_main` owns identity, relay client, and IPC buffer in a loop.
- VFS is the current attested-service reference pattern: it receives with `sys_recv_attested`, builds a `Caller`, handles request, and only then replies.
- Completion queue is per-cell and bounded, but not a substitute for local IPC caller attestation.

## Implication

Candidate B is viable without Law 1 because local attested ingress can be blocking in a dedicated task. Candidate A should remain a contingency for measured failure, not the default.
