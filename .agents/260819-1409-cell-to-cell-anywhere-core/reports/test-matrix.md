---
title: "Test Matrix"
status: pending
created: 2026-08-19
---

# Test Matrix

## Unit

- Node identity: first boot, reboot stable, malformed key, duplicate node id detection hook.
- Export registry: absent export, malformed export, retry class validation, version mismatch.
- Envelope: encode/decode round trip, unknown version, oversized payload, bad length.
- Dedup: duplicate before completion -> `Busy`; duplicate after completion -> cached response; stale epoch -> reject/indeterminate.
- Deadline: before delivery timeout, after delivery indeterminate, zero/overflow relative deadline.
- Error taxonomy: NoService, Unreachable, Timeout, Busy, Indeterminate, AuthFailed, ProtocolError.
- Dedup states: accepted, dispatched, completed, expired, exhausted.
- Typed API: `LocalEndpoint<M>` never routes through broker; `RemoteEndpoint<M>` returns typed remote errors; `CellEndpoint<M>` union requires deliberate match.

## Integration

- Local direct endpoint lookup bypasses broker.
- Remote lookup returns broker endpoint only for exported service.
- Broker ingress receives attested caller identity through blocking ingress task.
- Bounded local queue returns `Busy` when full.
- Scheduler prototype verifies ingress task, worker task, bounded wakeups, heartbeat, watchdog, and network polling coexist.
- Relay frame dispatch reaches C2C decoder.
- Direct path does not carry frames before Noise auth.
- Broker replies are correlated by request id under concurrent broker traffic.

## E2E Oracles

- Relay-only two-node exported call.
- LAN-direct two-node exported call after relay path passes.
- Direct break mid-request falls back to relay or returns `Indeterminate`.
- Relay outage returns `Unreachable`.
- Wrong node id/key fails authentication.
- Restarted broker changes epoch and protects dedup semantics.
- 10k accepted unary-call soak has zero silent drops and zero duplicate local dispatches inside retention window.
- Local direct IPC p99 regression is <=5% versus captured baseline.
- Concurrency and saturation targets are derived from measured broker baseline.
- Relay missing-destination and forward-write failure return sender-visible status.
- Path switch cannot reorder responses into a newer request id.

## Failure Injection

- Oversized frame.
- Replayed request id.
- Stale boot epoch.
- Queue saturation.
- Dedup expiry and dedup exhaustion.
- Relay disconnect.
- Direct TCP half-open.
- Broker restart while non-idempotent request is in-flight.
- Stale delayed response after timeout/retry.
- Duplicate NodeId relay registration/hijack attempt.
- Forbidden log content: payloads, PSKs, private keys, full request bodies.
- Wrong cluster id.
- Wrong Noise static key.

## Evidence Labels

- CI/unit evidence is not two-node runtime evidence.
- QEMU evidence is not hardware evidence.
- Hardware evidence is only claimed after an actual hardware run with retained logs.
- Production readiness is not claimed by this plan.
