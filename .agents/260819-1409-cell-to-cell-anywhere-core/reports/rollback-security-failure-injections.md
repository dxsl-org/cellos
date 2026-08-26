---
title: "Rollback Security and Failure Injections"
status: pending
created: 2026-08-19
---

# Rollback Security and Failure Injections

## Rollback Summary

- Phase 01: revert plan folder only.
- Phase 02: disable remote exports and keep local services.
- Phase 03: disable broker remote ingress queues and keep local direct IPC.
- Phase 04: reject remote endpoint requests at broker boundary.
- Phase 05: disable relay mode and keep local/LAN-disabled state.
- Phase 06: force relay path; direct path off.
- Phase 07: keep counters inert; remote exports disabled if status mapping is wrong.
- Phase 08: keep status partial; no rollout.
- Phase 09: do not start Candidate A unless gates are met.

## Security Gates

- Remote service must be explicitly exported.
- Export registry must be init/supervisor-provisioned, read-only to broker at runtime, atomically replaced, version-validated, and fail closed.
- Node identity must be stable and authenticated by Noise.
- Relay must be authenticated and self-hosted for V1 oracles.
- Payload bytes must be E2E encrypted across relay.
- Broker must not trust caller-provided identity.
- Logs must not include keys, PSKs, or plaintext payloads.

## Failure Injection Gates

- Auth: wrong key, wrong node id, wrong cluster id.
- Auth: duplicate NodeId relay registration/hijack attempt.
- Replay: same request id, stale boot epoch, stale server epoch.
- Replay: stale delayed response after timeout/retry.
- Transport: relay disconnect, direct disconnect, path switch during request.
- Transport: missing relay destination, relay write failure, half-open TCP.
- Restart: broker or peer reboot while non-idempotent request is in-flight.
- Backpressure: local ingress queue full, per-peer send queue full, dedup full.
- Dedup: expiry, exhaustion, accepted/dispatched/completed state recovery.
- Protocol: bad version, bad length, oversized payload, unknown export id.
- Timing: deadline expires before delivery, deadline expires after delivery.

## High Risks

- Hidden duplicate side effect under retry. Mitigation: retry class + dedup + `Indeterminate`.
- Blocking ingress causes latency cliff. Mitigation: dedicated task, bounded queues, Candidate A contingency after oracle.
- Relay becomes trusted. Mitigation: E2E Noise and no relay plaintext.
- Early COMPLETE claim. Mitigation: Phase 08 gate.
