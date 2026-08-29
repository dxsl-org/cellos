---
title: "Phase 04 - C2C Envelope Request Semantics and Dedup"
status: completed
priority: P1
effort: 5
depends_on: [02, 03]
owner: "c2c-protocol"
---

# Phase 04 - C2C Envelope Request Semantics and Dedup

## Context Links

- Semantics: `research/semantics-report.md`
- Test matrix: `reports/test-matrix.md`

## Overview

Priority P1. Define the V1 broker-only C2C envelope and exact request semantics before any LAN or relay path is treated as product behavior.

## Key Insights

- Remote cannot guarantee local IPC semantics under partition.
- `Busy` and `Indeterminate` are required to avoid lying about delivery.
- Dedup is scoped at-most-one local dispatch only inside an authenticated retention window.
- Typed client API belongs in focused `ostd::cluster_endpoint`; shared wire
  value types belong in `types::c2c`, never the kernel ABI.

## Requirements

- Functional: envelope encode/decode, typed endpoint API, request id, source boot epoch, target server epoch, relative deadline, retry class, bounded dedup cache, accepted/dispatched/completed state, and explicit overload behavior.
- Non-functional: fit in bounded message size; no streaming in V1; deterministic decode failures; local path is never brokered.

## Entry Gate

- Phase 03 scheduler coexistence, correlation, bounded wakeup, heartbeat, and network-polling prototype is complete.
- Error taxonomy, retry mapping, and no-evict-in-flight invariant are frozen here before relay work starts.
- Client API owner is `libs/ostd/src/cluster_endpoint.rs`.
- Qualified provider execution and retained recovery evidence from Phase 02
  block remote dispatch and enablement, not local protocol construction.
  Envelope/decode/dedup modules and host/RV64 proofs may proceed while every
  remote route stays disabled.

## Architecture

Data flow: `LocalEndpoint<M>` performs direct sender-masked IPC. `RemoteEndpoint<M>` carries authenticated route metadata only until a later enabled path constructs an envelope; no Phase 04 remote transmit method exists. Future receive flow remains peer decoder -> current-server-epoch check -> dedup accepted/dispatched/completed -> local delivery -> response cache -> response envelope.

## Related Code Files

- `cells/services/net-broker/src/c2c_envelope.rs`
- `cells/services/net-broker/src/c2c_envelope/tests.rs`
- `cells/services/net-broker/src/c2c_dedup.rs`
- `cells/services/net-broker/src/c2c_dedup/types.rs`
- `cells/services/net-broker/src/c2c_dedup/tests.rs`
- `cells/services/net-broker/src/server_epoch.rs`
- `cells/services/net-broker/src/c2c_deadline.rs`
- `cells/services/net-broker/src/c2c_receive.rs`
- `cells/services/net-broker/src/c2c_receive/tests.rs`
- `cells/services/net-broker/src/c2c_dedup/source_window.rs`
- `libs/types/src/c2c.rs`
- `libs/api/src/services/ipc.rs`
- `libs/ostd/src/cluster_endpoint.rs`
- `libs/ostd/tests/cluster-endpoint.rs`
- `libs/ostd/src/clients/net.rs`
- `cells/services/net-broker/src/transport.rs`
- Non-owner by default: no kernel ABI changes.

## Implementation Steps

1. Define binary envelope layout and versioning.
2. Define typed API: `LocalEndpoint<M>`, `RemoteEndpoint<M>`, and deliberate `CellEndpoint<M>` union with typed remote errors.
3. Define bounded dedup cache and eviction.
4. Define local delivery and response cache state machine.
5. Define fuzz/property tests for decode and dedup.
6. Define error taxonomy and retry mapping before relay work.

## Todo List

- [x] Cap V1 payload at the minimum bound after local ingress, Noise AEAD, and
  net-cell `TcpSend` IPC costs.
- [x] Fix dedup TTL at 30 seconds, cache capacity at 16 entries, and replay
  floors at 16 authenticated source windows.
- [x] Assign nonzero monotonically increasing request ids per source boot epoch;
  the receiver rejects missing ids at or below the authenticated high-water mark.
- [x] Scope server epochs to one broker incarnation, provide a fresh nonzero
  sequence for export registration, and define stale-target rejection.
- [x] Define `LocalEndpoint<M>`, metadata-only `RemoteEndpoint<M>`, and the
  deliberate `CellEndpoint<M>` union with typed remote outcomes.
- [x] Define and host-test dedup expiry and exhaustion behavior.

## Success Criteria

- Duplicate requests do not double-dispatch locally within authenticated `(src_node, src_boot_epoch, request_id, dst_server_epoch)` retention window.
- Known non-idempotent duplicates return `Indeterminate` after cached response
  expiry. A bounded authenticated source/boot high-water floor preserves that
  result after completed-entry eviction.
- Dedup saturation proves no in-flight request is evicted into duplicate dispatch.
- Deadline math is relative and monotonic.
- Decode failure is observable and does not panic.

## Risk Assessment

- Risk: retry semantics too complex. Likelihood medium, impact medium. Mitigation: three retry classes only.
- Risk: dedup cache exhaustion. Likelihood medium, impact high. Mitigation: accepted/dispatched/completed states, no duplicate dispatch in retention, and `Busy`/`Indeterminate` outside guarantees.
- Risk: monotonic high-water admission rejects reordered lower ids. Mitigation:
  V1 sends requests in order per authenticated source boot; path-transition
  reordering remains an explicit Phase 08 gate before remote enablement.

## Security Considerations

Envelope identity is authenticated by Noise path and checked against route state. It is not trusted before session auth. Boot-local server epochs cannot enable remote dispatch until authenticated session incarnation state invalidates endpoints learned from an older broker incarnation.

## Rollback

Disable remote endpoint exports. Local direct endpoint path continues because no local ABI changes are made.

## Next Steps

Phase 04 is complete at the disabled local-only ceiling. Phase 05 remains
blocked until qualified-provider and retained-recovery Phase 02 evidence plus
authenticated cross-broker session-incarnation binding permit remote dispatch;
relay-first and direct-LAN work remain closed.

## Local Protocol Evidence

- Canonical V1 encode/decode is fixed at a 112-byte header. The 3,712-byte
  payload cap accounts for local ingress, Noise AEAD, and net-cell `TcpSend`
  IPC bounds. V1 streaming and fragmentation remain absent.
- The 16-entry, 30-second cache never evicts in-flight entries. Duplicates
  replay retained responses, return `Busy` while in flight, and return
  `Indeterminate` when a retained non-idempotent response has expired.
- Sixteen authenticated source/boot windows retain monotonic request-id floors,
  so completed-entry reuse cannot redispatch an evicted old id. An observed
  newer boot advances its floor before response-capacity admission. Saturation
  returns `Busy`; expired completed entries release response-cache capacity.
- `ServerEpochSource` provides fresh nonzero values within one broker lifetime.
  Future export registration must issue one per successful server incarnation;
  `require_current` rejects a replaced target before future dedup/local delivery.
  Cross-broker endpoint invalidation remains an explicit remote-enable
  session-binding gate.
- `LocalEndpoint<M>` calls direct sender-masked IPC. `RemoteEndpoint<M>` exposes
  authenticated metadata and retry policy but no transmit method;
  `CellEndpoint<M>` requires an explicit locality branch. The obsolete raw
  `ClusterRef` facade was removed because its broker request path never replied.
- This slice is not wired to remote dispatch and makes no provider, relay,
  two-node, or direct-LAN claim.
- Hostile mutation/re-encode properties pass for the canonical decoder.
  `RelativeDeadline` makes a nonzero caller budget mandatory in both the
  envelope and `RemoteEndpoint::call`; tests cover zero wire/API rejection,
  overflow, and exact expiry before/after dispatch. `ReceiveGate` rejects
  stale/non-request input before dedup, rejects same/lower replacement epochs
  without mutation, and retires dead-incarnation response entries on increasing
  replacement while retaining authenticated request-id floors.
- Host evidence: shared types 54/54, endpoint integration 5/5, and focused broker
  tests 92/92 pass. RV64 release builds for the broker and `ostd` pass. Tester
  and production-readiness reviewer rechecks pass.
