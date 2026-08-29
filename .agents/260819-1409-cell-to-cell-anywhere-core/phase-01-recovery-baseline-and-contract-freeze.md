---
title: "Phase 01 - Recovery Baseline and Contract Freeze"
status: complete
priority: P1
effort: 3
depends_on: []
owner: "architecture"
---

# Phase 01 - Recovery Baseline and Contract Freeze

## Context Links

- Plan: `plan.md`
- Audit: `research/research-audit.md`
- Scout: `reports/scout-report.md`

## Overview

Priority P1. Freeze Candidate B and mark this folder as the recovery plan superseding `.agents/260624-cell-to-cell-anywhere/` without editing that older plan.

## Key Insights

- D38 says completion is false until a two-node remote-call oracle exists.
- Current `net-broker` has useful modules but remote dispatch is still not wired.
- Law 1 must be avoided unless Candidate B fails its oracle.

## Requirements

- Functional: define Local endpoint, Remote endpoint, export registry, V1 C2C envelope, typed remote errors.
- Non-functional: no product-code edits in this phase; record only the bounded single-guest local-runtime QEMU CI evidence, with no two-node, relay, LAN, hardware, or production claim.

## Frozen Feasibility Budgets

- Capture pre-change local direct IPC p99 baseline; post-change local direct IPC p99 regression must be <= 5%.
- Broker runtime must show zero watchdog expirations in the declared oracle windows.
- Queue and dedup-cache memory budgets must be recorded before implementation.
- 10k accepted unary-call soak must complete with zero silent drops and zero duplicate local dispatches inside the declared retention window.
- Concurrency and queue-saturation targets must be derived from a measured broker baseline, not an invented absolute throughput number.

## Architecture

Data flow: old artifacts and current code evidence enter as PRIOR/current inputs -> plan authority is reset -> outputs are phase gates, assumptions, and file ownership.

## Related Code Files

- Read-only evidence: `cells/services/net-broker/src/main.rs`, `cells/services/net-broker/src/routing.rs`, `cells/services/net-broker/src/relay.rs`
- Planned future ownership: none in this phase.

## Implementation Steps

1. Confirm `.agents/260624-cell-to-cell-anywhere/` is superseded, not modified.
2. Adopt Candidate B in plan frontmatter and phase sequence.
3. Record Candidate A as contingency only.
4. Freeze V1 defer list.
5. Record local IPC baseline, watchdog target, memory budgets, soak target, and measured concurrency target before any implementation phase starts.

## Todo List

- [x] Root validates the evidence citations.
- [x] Root completed Red Team Review before implementation.
- [x] User confirms Law-1-free Candidate B is the default.
- [x] Capture and CI-gate the pre-change local direct IPC p99 baseline.
- [x] Record and CI-gate the queue/cache memory budgets.
- [x] Record and CI-gate the measured broker concurrency and saturation baseline.

## Success Criteria

- Plan names recovery status and explicit supersedence.
- No old "complete" claim remains in this folder.
- Candidate A has hard entry gates.
- Release budgets are frozen and measurable before implementation starts.

## Risk Assessment

- Risk: stale old plan gets implemented accidentally. Likelihood medium, impact high. Mitigation: this phase names the superseding folder and leaves old plan untouched.
- Risk: Candidate B hides a scheduler issue. Likelihood medium, impact high. Mitigation: Phase 03 prototype must fail reproducibly against frozen budgets before Candidate A can be considered.
- Risk: throughput targets become invented. Likelihood medium, impact high. Mitigation: concurrency and saturation targets are measured from broker baseline.

## Security Considerations

Remote is never trusted as local. Cluster id is routing metadata only; node auth belongs to Noise and stable node keys.

## Rollback

Revert this plan folder only. No product code or docs outside the folder are touched.

## Next Steps

Proceed to stable node identity and export registry. Candidate A triggers only after reproducible failure against frozen targets with root cause specifically blocking ingress and no userspace correction.

## Deviation Log

- **2026-08-27**: Implemented Relay-First Contract Freeze.
  - Explicitly added `NotSupported` to `ReplyStatus` in `local_ingress.rs`.
  - Updated dispatch to return `NotSupported` instead of `Indeterminate` when paths are exhausted in `request_dispatch.rs`.
  - Documented the freeze of the Relay-First contract in `main.rs`.
  - Validated changes via bench_oracle/frames compilation scope. All changes confined within broker.
- Correction: the contract-freeze work added a fail-closed `NotSupported`
  outcome before the required feasibility measurements. That behavior is
  retained as compatible with the user-selected relay-first contract, but it
  does not complete this phase or authorize later runtime work.
- Resolved local prerequisite: the ephemeral run-scoped K1 image fixture and
  single-guest baselines are implemented and required by CI job
  `c2c-broker-oracle-single-guest-local-runtime`. The actual isolated RV64 QEMU
  runner passed 1/1 with `samples=1000 success=1000 calibration=MEASURED`,
  `role_gate=PASS`, successful 1/2/4/8/16-client sweeps,
  `soak attempted=10000 success=10000 silent_drop=0`, and
  `overflow status=PASS` at `queue_peak=16`.
- Evidence boundary: the CI job proves only the single-guest local broker/runtime
  QEMU oracle, including supervised local broker restart. Two-node relay,
  direct-LAN, remote in-flight restart/failover, Phase 05, and production remain
  open.
- Governance gate: only the two-real-broker relay path is blocked by the
  protected-persistence, authenticated-time, and reviewed pending-key-binding
  entry gates under frozen KMS opcodes 9–14 in
  `.agents/260825-1726-kms-silo-production-root/phase-04-service-net-mutual-tls-integration.md`.
  Reopen that path only when DEV_REFERENCE Phase 8's exact verifier emits
  `GO: PHASE4_ENTRY_GATES_SATISFIED`; this does not block approved single-guest
  or other local-only work.
- Governance review: the attempted protocol scaffold for the blocked two-broker
  path was fully reverted; no dead or unwired implementation remains.
