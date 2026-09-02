---
phase: 3
title: "Local QEMU Oracle and Evidence Sync"
status: completed
priority: P1
effort: 0.5d
dependencies: [2]
tier: medium
---

# Phase 03: Local QEMU Oracle and Evidence Sync

> **Required — deviation-log:** Record each Decision / Deviation / Surprise immediately. Choose the smallest reversible response; do not convert QEMU timing into a physical-latency claim.

## Overview

Prove the queued-IPC completion wake with the kernel's deterministic QEMU selftests, then independently gate the unchanged benchmark after a fresh command `START`. Service-net raw-zero timing remains supplemental observation only because a later mailbox drain cannot establish what caused the earlier recordless return.

## Requirements

- Reuse the existing broker-to-net `sys_post` request path and `bench c2c-broker-oracle`; add no completion ABI, request opcode, remote relay, or cross-guest behavior. The review-added detailed OSTD result seam is the only API-level scope deviation.
- Require the exact kernel `IPC-PENDING` completion-wake and `NET-RX-RESERVATION` IPC-safe PASS markers with neither corresponding FAIL marker before launching the benchmark.
- Emit `idle_ipc_wake status=PASS` only for exact raw `0` followed by a same-cycle drain strictly below the exclusive 900,000-tick proof ceiling. Emit `status=INCONCLUSIVE wake=recordless raw_ret=0 reason=late-drain` at or above that ceiling; this is neither PASS nor FAIL.
- Do not delay benchmark launch waiting for a runtime timing PASS. At final whole-run parsing, require at least one exact validated PASS; well-formed INCONCLUSIVE results are neutral, but INCONCLUSIVE-only output cannot pass the run. Reject genuine `FAIL`, `BLOCKED`, or legacy maintenance-timeout markers across the whole run.
- After the deterministic kernel checkpoint, issue the unchanged benchmark command and require a fresh subsequent `START`.
- Retain every existing oracle gate: real NET_RX proof, measured calibration, 1/2/4/8/16 sweeps, 10,000/10,000 soak with zero silent drops, positive network progress, zero heartbeat/watchdog deltas, overflow pass, and restart pass.
- Oracle instrumentation is feature-gated and absent from normal service-net builds; it must not change the one-yield grace, scheduling, syscall count, wait duration, or messages.

## Architecture

The `ipc-wake-oracle` service-net feature is observation-only. A bounded module records the wait-cycle number and `sys_get_time` immediately before the real completion wait. A detailed OSTD decoder separates exact raw `0`, a valid completion, and error/invalid results; only exact raw `0` can become a candidate. A same-cycle drain below the exclusive 900,000-tick ceiling is an exact PASS observation required somewhere in the full retained run. A drain at or above the ceiling is INCONCLUSIVE because it may follow the normal maintenance wake; it neither satisfies nor fails the run.

The canonical causal proof comes from the exact kernel `IPC-PENDING` completion-wake and `NET-RX-RESERVATION` IPC-safe selftests. Once those PASS with no corresponding FAIL, the host checkpoints output and immediately launches the benchmark, then requires the fresh `START` and every broad result gate. Final parsing additionally requires at least one exact runtime PASS across the retained output. That timing observation is mandatory but non-causal: it does not establish that the benchmark or queued IPC caused a particular raw-zero return. Feature-on retains the production grace yield of one.

## Assumptions

- **Claim:** The isolated runner can rebuild service-net with one package feature without changing the broker/bench feature set or image layout.
  **Confidence:** high
  **How to verify:** The phase command must build the feature-enabled service-net in the existing private `CARGO_TARGET_DIR` and confirm the runner's existing artifact checks.
- **Claim:** Guest `sys_get_time` and `SMOLTCP_MAINTENANCE_TICKS` share the mtime unit used by the service loop.
  **Confidence:** high
  **How to verify:** Keep both reads inside service-net and assert the existing host maintenance test remains 1,000,000 mtime ticks.

## Related Files

- Modify: `cells/services/net/Cargo.toml` — private `ipc-wake-oracle` feature.
- Modify: `cells/services/net/src/main.rs` — feature-gated oracle module declaration.
- Create: `cells/services/net/src/idle-ipc-wake-oracle.rs` — bounded cycle/elapsed observation, under 200 lines.
- Modify: `cells/services/net/src/service-runtime.rs` — feature-gated before-wait, detailed raw-result, and IPC-drained hooks.
- Modify: `libs/ostd/src/syscall.rs` and focused tests — detailed wait-result decoder with a compatible legacy wrapper.
- Modify: `libs/ostd/src/clients/vfs/read_file/path.rs` — private-type derives needed for the pre-existing package tests to compile; no layout or visibility change.
- Modify: `scripts/run-c2c-broker-oracle-qemu.sh` — build service-net with the oracle feature in the isolated image.
- Modify: `tests/integration/tests/c2c-broker-oracle.rs` — deterministic kernel checkpoint, exact supplemental runtime-result validation, whole-run failure rejection, independent command `START`, and retained baseline assertions.
- Previously modified for the pre-final evidence correction: `docs/roadmap/open-risk-register.md`, `docs/roadmap/current-focus.md`, `docs/project-roadmap.md`, `docs/project-changelog.md` — qualify the earlier cycle 36 observation rather than presenting it as final-source proof.

## Implementation Steps

1. Add the feature and bounded focused module. Track only cycle, wait-start tick, raw result classification, and the current candidate; reset or clear it on every disqualifying outcome.
2. Emit `ARMED` immediately before the real wait with coherent cycle/start/budget/ceiling fields. Emit PASS only for an exact raw-zero same-cycle drain below 900,000 ticks; emit well-formed INCONCLUSIVE for a late drain.
3. Add a detailed OSTD wait-result decoder that distinguishes `NoRecord`, valid `Completion`, and `ErrorOrInvalid`, while preserving the legacy `Option` wrapper.
4. Keep the real wait at 10 scheduler ticks, the post-reply grace at one, and the feature off by default. Confirm normal RV64 artifacts contain no oracle markers.
5. In the host test, require the exact kernel completion-wake and IPC-safe PASS markers with no corresponding FAIL before the command checkpoint. Reject `FAIL`, `BLOCKED`, and maintenance-timeout markers from the whole run.
6. Launch the unchanged benchmark without waiting for a timing PASS, require a fresh later `START`, and evaluate the retained NET_RX, calibration, sweep, soak, network-progress, liveness, overflow, and restart gates.
7. Validate every retained timing PASS as exact raw zero, recordless, same-cycle, and strictly below the ceiling, and require at least one such PASS in final whole-run parsing. Accept individual INCONCLUSIVE markers as neutral evidence, but reject INCONCLUSIVE-only runs.
8. Keep evidence wording local and software/QEMU-only. Distinguish the earlier cycle 36 observation from evidence recorded by the successful clean-source canonical run.

## Success Criteria

- [x] The canonical gate requires exact kernel `IPC-PENDING` completion-wake and `NET-RX-RESERVATION` IPC-safe PASS markers with no corresponding FAIL, launches the benchmark without first waiting for timing PASS, and finally requires at least one exact runtime PASS.
- [x] Classifier boundaries encode 899999 as PASS and 900000, 1000000, and 1473679 as INCONCLUSIVE; a late raw-zero drain is neither run success nor failure, while INCONCLUSIVE-only output cannot pass the run.
- [x] The independent post-checkpoint benchmark still requires a fresh `START`, real NET_RX proof, measured 1000/1000 calibration, 1/2/4/8/16 sweeps, 10000/10000 soak, positive network progress, zero heartbeat/watchdog deltas, overflow, and restart.
- [x] Genuine `FAIL`, `BLOCKED`, and legacy maintenance-timeout markers remain rejected across the whole run.
- [x] The final-source canonical runner passed at clean commit `59501e2b29a7000004249977073c8069e5a67fa6`: exact kernel selftest markers, mandatory exact runtime PASS, and every benchmark result were retained.
- [x] Risk, current-focus, project-roadmap, and changelog projections qualify the earlier cycle 36 capture as pre-final-code rather than treating it as final-source proof.

## Evidence and Results

- Earlier API, service-net, feature-off artifact, detailed-decoder, and exact kernel boot-test results remain recorded in their owning phases.
- Cycle 36 (`start_ticks=144542529 raw_ret=0 elapsed_ticks=442232`) was captured before the final oracle/classifier source. It is a historical supplemental observation, not final-source proof and not evidence that queued IPC caused that raw-zero return.
- The first final-source attempt stopped at startup cycle 30 when the old classifier reported a late raw-zero drain as legacy `maintenance-timeout`. That outcome motivated INCONCLUSIVE classification; it is not a runtime failure finding and did not complete the canonical benchmark gate.
- The corrected canonical runner was invoked exactly once at source commit `59501e2b29a7000004249977073c8069e5a67fa6`, with the tree clean at that commit before and after. It exited 0 and reported 1/1 passing. Captured output contained `[selftest] IPC-PENDING: PASS (deferred, bounded, quota-safe, completion-wake)` and `[selftest] NET-RX-RESERVATION: PASS (fills, remembers, releases, IPC-safe)` with no corresponding FAIL.
- The mandatory supplemental runtime result was `idle_ipc_wake status=PASS cycle=36 wake=recordless raw_ret=0 start_ticks=144911300 elapsed_ticks=586804 budget_ticks=1000000 proof_ceiling_ticks=900000`. It was same-cycle and strictly below the exclusive local/QEMU ceiling; no INCONCLUSIVE marker appeared. This observation is non-causal: the deterministic kernel selftests, not service-net timing, prove completion wake.
- A fresh benchmark `START` followed the checkpoint. `[net-rx-producer] irq->completion PASS` established the real NET_RX gate; measured calibration completed 1000/1000; the role gate passed; the 1/2/4/8/16 sweeps passed; and the soak completed 10000/10000 with zero silent drops. Network progress was positive; heartbeat-miss and watchdog-expired deltas were zero; overflow and restart passed. No forbidden marker appeared: no oracle `FAIL`, `BLOCKED`, or legacy maintenance-timeout; kernel-selftest FAIL; restart role-drain or IPC-admission timeout; beacon IPC timeout/network-disabled marker; or heartbeat/watchdog task termination.
- Evidence ceiling remains local software plus isolated single-guest QEMU only. The mandatory runtime timing PASS is non-causal and creates no remote, external-system, physical-latency, deployment, or production claim.

## Security Considerations

The feature must be off by default, expose no syscall/request surface, allocate no unbounded state, and log no payload or caller identity. The runner remains isolated and keeps its existing private key/image handling. Documentation must not overstate QEMU evidence as remote or hardware qualification.

## Risk Notes

An `ARMED` line is an observation, not a reservation: autonomous broker traffic shares the principal and can replace or consume a cycle. More importantly, a later TryRecv drain cannot causally attribute the preceding raw-zero return. The deterministic kernel selftests therefore own the completion-wake proof. The gate launches the benchmark after those selftests without waiting for timing, but final whole-run parsing still requires one exact sub-ceiling PASS; individual late drains remain neutral INCONCLUSIVE and cannot satisfy that requirement. The fresh post-checkpoint `START` and broad benchmark gates independently prove the benchmark run. Exact raw-result decoding, the exclusive ceiling, and whole-run rejection of genuine failure markers close false-positive and false-negative paths without turning ambiguity into failure.

## Deviation Log

- **Observation-only oracle deviation:** Rejected the planned host-selected fresh post-shell cycle because `ARMED` provides no lease or acknowledgement and autonomous broker traffic uses the same service principal. Restored the production post-reply grace of one for feature-on builds.
- **Non-causal timing correction:** Final-source review proved that a late raw-zero first drain is ambiguous: it may follow maintenance and cannot identify the earlier wake cause. Reclassified elapsed time at or above 900,000 ticks as INCONCLUSIVE, made the exact kernel completion-wake/IPC-safe selftests the canonical launch prerequisite, and removed the pre-launch wait for timing PASS. Final whole-run parsing still requires at least one exact early PASS; INCONCLUSIVE-only output cannot pass.
- **Evidence correction:** The earlier cycle 36 and its 442232-tick drain came from pre-final-code source and had been prematurely labeled final proof. The living documents identify it only as historical supplemental evidence. The first final-source attempt stopped at cycle 30 on the legacy maintenance-timeout classification and remains recorded as an incomplete attempt; the corrected clean-source canonical run later passed at commit `59501e2b29a7000004249977073c8069e5a67fa6`.
- **Review-driven hardening retained:** Exact raw `0`, the unchanged 1,000,000-tick maintenance budget, the exclusive 900,000-tick PASS ceiling, whole-run rejection of genuine failure markers, and all benchmark gates remain mandatory.
- **OSTD baseline caveat:** Private trait derives allowed the detailed decoder tests to compile. The focused decoder test passed, but the package remains 23/24 due the unrelated pre-existing `read_file` bounds failure; it is not reported as a pass.