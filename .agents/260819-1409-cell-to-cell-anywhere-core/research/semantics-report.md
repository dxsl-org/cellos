---
title: "C2C Semantics Report"
status: pending
created: 2026-08-19
---

# C2C Semantics Report

## Verdict

V1 defines Cell-to-Cell Anywhere as a typed endpoint contract, not as remote IPC pretending to be local IPC. Local calls stay direct kernel IPC; remote calls pass through `net-broker` and expose typed remote failures.

## Endpoint Model

- Local endpoint: `(service_id, local_tid, export_policy)` resolved by existing service lookup and called directly.
- Remote endpoint: `(node_id, service_id, export_id, cluster_id)` resolved by broker routing and called through C2C frames.
- Export registry: services are not remotely callable by default. A service must export a named endpoint and retry class.
- Auth boundary: `ClusterId` filters routes only; Noise key possession authenticates node-level membership. `ClusterId` is explicitly not a credential: `libs/api/src/services/cluster.rs:8`.

## C2C Envelope

Fields: `version`, `kind`, `request_id`, `src_node`, `dst_node`, `src_boot_epoch`, `dst_server_epoch`, `cluster_id`, `service_id`, `export_id`, `relative_deadline_ms`, `retry_class`, `flags`, `payload_len`, `payload`.

Kinds: lookup, request, response, busy, indeterminate, cancel, heartbeat.

## Request Semantics

- Normal success: request delivered once locally, response cached until dedup TTL.
- Duplicate request: broker returns cached response or `Busy` if first copy is still executing.
- Unknown completion after path failure: return `Indeterminate` for retry classes that cannot be safely replayed.
- Deadline expired before local delivery: return timeout without sending to target service.
- Deadline expired after local delivery: return `Indeterminate`, unless the target already produced a response.
- Target busy/backpressure: return `Busy { retry_after_ms }`, not a transport error.

## Dedup Bound

- Cache key: `(src_node, src_boot_epoch, request_id)`.
- Cache value: status, response/error bytes, first-seen monotonic time, target service, retry class.
- Bound: fixed entries per peer and fixed bytes total; eviction prefers completed expired entries, then oldest idempotent entries.
- Restart rule: broker boot epoch change invalidates the cache; callers treat stale epoch as `Indeterminate` unless retry class allows replay.

## Data Flow

Local caller -> endpoint resolver -> local tid fast path OR broker remote path -> C2C envelope -> path sender -> remote broker -> attested local delivery -> response cache -> reverse path -> caller.

## Failure Modes

- Duplicate delivery after reconnect: mitigated by request id + boot epoch + bounded dedup.
- Silent remote hang: mitigated by relative deadline and `Indeterminate`.
- Remote service not exported: return `NoService`, not a raw timeout.
- Replay from old node boot: rejected by boot epoch window.
- Misleading local equivalence: prevented by explicit Remote endpoint type.

## Tests Required

Unit: envelope encode/decode, duplicate cache, retry-class transitions, deadline math. Integration: local export lookup, remote lookup, Busy, Indeterminate, dedup hit. E2E oracle: relay-only two-node request, LAN-direct request, relay fallback after direct break.
