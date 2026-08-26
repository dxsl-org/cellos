---
title: "Phase 04 - C2C Envelope Request Semantics and Dedup"
status: pending
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
- Typed client API belongs in `libs/ostd/src/cluster.rs` or a focused `ostd` module, not `libs/api` or kernel ABI by default.

## Requirements

- Functional: envelope encode/decode, typed endpoint API, request id, source boot epoch, target server epoch, relative deadline, retry class, bounded dedup cache, accepted/dispatched/completed state, and explicit overload behavior.
- Non-functional: fit in bounded message size; no streaming in V1; deterministic decode failures; local path is never brokered.

## Entry Gate

- Phase 03 scheduler coexistence, correlation, bounded wakeup, heartbeat, and network-polling prototype is complete.
- Error taxonomy, retry mapping, and no-evict-in-flight invariant are frozen here before relay work starts.
- Client API owner is assigned to `libs/ostd/src/cluster.rs` or a focused `ostd` module.

## Architecture

Data flow: typed client endpoint -> local direct IPC for `LocalEndpoint<M>` OR envelope builder for `RemoteEndpoint<M>` -> path sender -> peer decoder -> dedup state accepted/dispatched/completed -> local delivery -> response cache -> response envelope.

## Related Code Files

- Future owner phase: new protocol module under `cells/services/net-broker/src/`
- Future owner phase: `cells/services/net-broker/src/routing.rs`
- Future client API owner: `libs/ostd/src/cluster.rs` or focused `libs/ostd/src/cluster_endpoint.rs`
- Future non-owner by default: no `libs/api` or kernel ABI changes.

## Implementation Steps

1. Define binary envelope layout and versioning.
2. Define typed API: `LocalEndpoint<M>`, `RemoteEndpoint<M>`, and deliberate `CellEndpoint<M>` union with typed remote errors.
3. Define bounded dedup cache and eviction.
4. Define local delivery and response cache state machine.
5. Define fuzz/property tests for decode and dedup.
6. Define error taxonomy and retry mapping before relay work.

## Todo List

- [ ] Choose max payload size.
- [ ] Choose dedup TTL and capacity.
- [ ] Choose request id generation source.
- [ ] Define server epoch persistence or boot-only scope.
- [ ] Define `LocalEndpoint<M>`, `RemoteEndpoint<M>`, and `CellEndpoint<M>` API shape.
- [ ] Define dedup expiry and exhaustion oracle.

## Success Criteria

- Duplicate requests do not double-dispatch locally within authenticated `(src_node, src_boot_epoch, request_id, dst_server_epoch)` retention window.
- Outside the retention window or after eviction, non-idempotent requests return `Indeterminate`.
- Dedup exhaustion oracle proves no retained non-idempotent request is evicted into duplicate dispatch.
- Deadline math is relative and monotonic.
- Decode failure is observable and does not panic.

## Risk Assessment

- Risk: retry semantics too complex. Likelihood medium, impact medium. Mitigation: three retry classes only.
- Risk: dedup cache exhaustion. Likelihood medium, impact high. Mitigation: accepted/dispatched/completed states, no duplicate dispatch in retention, and `Busy`/`Indeterminate` outside guarantees.

## Security Considerations

Envelope identity is authenticated by Noise path and checked against route state. It is not trusted before session auth.

## Rollback

Disable remote endpoint exports. Local direct endpoint path continues because no local ABI changes are made.

## Next Steps

Proceed to relay-first correctness oracle.
