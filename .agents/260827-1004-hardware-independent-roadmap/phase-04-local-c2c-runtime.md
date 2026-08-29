---
phase: 4
title: "Reconcile and Complete Local Cell-to-Cell Runtime"
status: in_progress
priority: P1
effort: "5d"
dependencies: [1]
tier: thinking
---

# Phase 04: Reconcile and Complete Local Cell-to-Cell Runtime

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.

## Context Links

- `docs/roadmap/current-focus.md`
- `.agents/260825-sdk-delivery/plan.md`
- `.agents/260819-1409-cell-to-cell-anywhere-core/plan.md`
- `cells/services/net-broker/src/main.rs`

## Overview

Reconcile the newer direct-only runtime with the pending relay-first recovery plan, then implement only the explicitly approved local software slice.

## Key Insights

Current code owns fail-closed K1 loading, authenticated beacons, and bounded
local roles, but does not construct either direct Noise sessions or relay
routing. The selected relay-first recovery contract is therefore the sole
implementation authority; direct LAN remains an optimization after its relay
oracle.

## Requirements

- The relay-first recovery plan is authoritative for transport ordering.
- Keep K1 loading and authenticated beacon setup fail closed.
- Freeze a bounded local test scope; public export and distributed leases remain
  deferred unless separately approved.
- Bound peer/session/routing state, frames, retries, timeouts, and restart
  cleanup.

## Architecture
After the recovery-plan contract freeze:
`K1 → authenticated discovery → relay-first end-to-end Noise → bounded
enrollment/routing state`; direct LAN follows only as an optimization. Exhausted
approved paths return `NotSupported`; no raw or insecure fallback exists.

## Assumptions
- **Decision:** User selected the relay-first recovery contract on 2026-08-27.
  **Implementation authority:** `.agents/260819-1409-cell-to-cell-anywhere-core/plan.md`.
  **Bounded scope:** private/test relay oracle plus local runtime; public export
  and distributed leases remain deferred.
- **Claim:** Two QEMU guests can exchange supported LAN traffic.
  **Confidence:** medium
  **How to verify:** run a minimal two-guest network smoke after the relay-first
  transport is wired; do not add a fake transport.

## Related Files

- Modify after contract approval: `cells/services/net-broker/src/main.rs`, `local_runtime.rs`
- Modify only if approved: `enrollment.rs`, `routing.rs`, `gossip.rs`
- Preserve as deferred: `cells/services/net-broker/src/lease.rs`
- Emit: focused two-node evidence for Phase 08; do not edit shared status ledgers

## Implementation Steps

1. Compare relay-first recovery, newer SDK cutover, current code, and production-identity blocks.
2. Explicitly supersede/revise one contract; do not leave both executable.
3. Freeze a local scope excluding distributed leases and public relay unless approved separately.
4. Wire only approved state into one runtime owner.
5. Exercise two QEMU guests for approved discovery/session/routing behavior and restart cleanup.
6. Confirm exhausted direct paths return `NotSupported` without insecure fallback.

## Todo List

- [x] Resolve relay-first versus direct-only contract conflict.
- [x] Freeze one bounded local runtime scope with distributed leases deferred.
- [x] Approve and implement the ephemeral run-scoped K1 image-fixture contract.
- [x] Record and CI-gate single-guest calibration, concurrency, soak, role,
  overflow, watchdog-log, and supervised local broker-restart evidence.
- [ ] Prove approved two-node relay/direct-LAN behavior and remote
  session/route restart cleanup.

## Success Criteria

- [ ] One authoritative C2C plan owns the runtime contract.
- [ ] Approved modules are reachable from one state owner without duplicate machinery.
- [ ] Two QEMU nodes complete the approved authenticated direct-LAN path.
- [ ] Restart removes stale sessions/routes and never enables raw relay fallback.

## Security Considerations

Authenticate before state mutation; bind cluster/node/session generation; cap every table and frame.

## Risk Assessment

Do not broaden into distributed leases, HyParView/PlumTree, internet traversal, public exports, or relay identity.

The K1 fixture and single-guest baseline oracle are implemented and required in
CI. This closes the local fixture, bounded runtime, and supervised local
broker-restart prerequisite only. Relay, two-node direct-LAN, and remote
session/route restart cleanup remain open; no raw or insecure fallback is
permitted.

## Deviation Log

- Decision: the user selected an ephemeral, run-scoped 32-byte K1 injected into
  the RV64 `app-bench` oracle image only; it is shared only by that run's
  participants and removed with its workspace.
- Evidence: required CI job
  `c2c-broker-oracle-single-guest-local-runtime` (`C2C Broker Oracle
  (single-guest local-runtime QEMU)`) runs
  `scripts/run-c2c-broker-oracle-qemu.sh`. The workflow YAML is valid, and the
  actual isolated RV64 QEMU runner passed 1/1 with
  `samples=1000 success=1000 calibration=MEASURED`,
  `role_gate=PASS`, successful 1/2/4/8/16-client sweeps,
  `soak attempted=10000 success=10000 silent_drop=0`,
  positive soak network progress, no kernel heartbeat/watchdog termination
  marker, `overflow status=PASS` at `queue_peak=16`, and `restart status=PASS`
  after clean role drain, stale old-TID rejection, fresh state, and retry on a
  replacement TID.
- Boundary: this is a single-guest local broker/runtime and supervised-process
  restart oracle. It does not satisfy two-node relay, direct-LAN, remote
  session/route restart cleanup, remote/public, Phase 05, or production
  criteria, and it does not raise this phase's evidence ceiling.
- Governance gate: only the two-real-broker relay path is blocked by the three
  entry gates in
  `.agents/260825-1726-kms-silo-production-root/phase-04-service-net-mutual-tls-integration.md`:
  protected persistence, authenticated time, and reviewed pending-key binding
  under frozen KMS opcodes 9–14 with AC-001 through AC-011 evidence. Reopen that
  path only when DEV_REFERENCE Phase 8's exact verifier emits
  `GO: PHASE4_ENTRY_GATES_SATISFIED`; this is not a global blocker for
  single-guest or other approved local work.
- Governance review: the attempted protocol scaffold for the blocked
  two-broker path was fully reverted. No dead or unwired implementation remains.
- Stable-identity slice: the broker now consumes the existing opaque KMS
  static-DH seam. Exact register/status/acquire snapshot agreement is mandatory;
  the private scalar never enters broker or VFS state. KMS absence/non-readiness
  falls back only to an ephemeral local identity with remote disabled.
  Plaintext VFS `machine-id` was explicitly rejected as a C2C identity root.
- Verification: focused host tests pass 63/63, RV64 broker/VFS release builds
  pass, and the rebuilt single-guest oracle passes 1/1 with measured
  calibration, concurrency, 10,000-call soak, role, overflow, watchdog-log,
  and supervised local broker-restart contracts. This does not close the open
  two-node or remote restart/failover criteria.
- Local protocol continuation: Phase 02 provider gates block remote dispatch,
  not bounded local envelope/decode/dedup construction. The 112-byte V1 header,
  3,712-byte end-to-end payload cap, 16-entry/30-second no-evict-in-flight
  cache, authenticated replay floors, boot-local server epochs, explicit typed
  endpoints, monotonic deadlines, and epoch-before-dedup receive ordering pass
  92/92 focused broker tests, 5/5 endpoint integration tests, and RV64 builds.
  The core Phase 04 protocol contract is complete at its disabled local-only
  ceiling. Remote transmit, relay, and direct LAN remain unimplemented.
